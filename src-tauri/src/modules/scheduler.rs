use chrono::{Local, Timelike};
use std::sync::Mutex;
use tokio::time::{self, Duration};

use crate::modules::{config, logger, quota};

// 防止同一分钟内重复触发
static LAST_TRIGGER_TIME: Mutex<Option<String>> = Mutex::new(None);

// 预热任务队列：支持同时跟踪多个高峰的预热任务
static WARMUP_QUEUE: Mutex<Vec<WarmupTask>> = Mutex::new(Vec::new());

// 上次预热成功时间（用于检测冷却结束）
static LAST_WARMUP_SUCCESS: Mutex<Option<i64>> = Mutex::new(None);

/// 预热任务（使用时间戳，支持跨日）
#[derive(Clone, Debug)]
struct WarmupTask {
    trigger_ts: i64,      // 触发时间戳
    peak_ts: i64,         // 高峰时间戳
    task_id: String,      // 唯一标识，如 "2026-01-10_14:30"
    status: WarmupStatus, // 任务状态
    retry_count: u32,     // 重试次数
}

/// 预热任务状态
#[derive(Clone, Debug, PartialEq)]
enum WarmupStatus {
    Pending,         // 等待执行
    WaitingCooldown, // 模型在冷却中，等待冷却结束
    Completed,       // 已完成
}

pub fn start_scheduler() {
    tauri::async_runtime::spawn(async {
        logger::log_info(
            "Smart Scheduler started with queue support. Checking for peak usage periods...",
        );
        let mut interval = time::interval(Duration::from_secs(60)); // Check every minute

        loop {
            interval.tick().await;

            let now = Local::now();
            let current_weekday = now.format("%a").to_string().to_lowercase();
            let current_timestamp = now.timestamp();

            let next_day = now.checked_add_signed(chrono::Duration::days(1)).unwrap();
            let next_weekday = next_day.format("%a").to_string().to_lowercase();

            if let Ok(app_config) = config::load_app_config() {
                if app_config.scheduled_warmup.enabled {
                    // ===== 步骤 1: 检查是否有新的高峰需要加入队列 =====
                    let check_and_add_tasks =
                        |day_key: &str, target_date: chrono::DateTime<Local>| {
                            let ranges = app_config
                                .scheduled_warmup
                                .schedules
                                .get(day_key)
                                .or_else(|| app_config.scheduled_warmup.schedules.get("default"));

                            if let Some(ranges) = ranges {
                                for range in ranges {
                                    if !range.enabled {
                                        continue;
                                    }

                                    if let (Ok(start_min), Ok(end_min)) =
                                        (parse_time_str(&range.start), parse_time_str(&range.end))
                                    {
                                        let mid_min = (start_min + end_min) / 2;
                                        let trigger_offset_min = mid_min - 300; // -5 hours

                                        // 计算高峰时间戳（target_date 的 mid_min 时刻）
                                        let peak_ts = target_date
                                            .date_naive()
                                            .and_hms_opt(
                                                (mid_min / 60) as u32,
                                                (mid_min % 60) as u32,
                                                0,
                                            )
                                            .map(|dt| {
                                                dt.and_local_timezone(Local).unwrap().timestamp()
                                            })
                                            .unwrap_or(0);

                                        // 计算触发时间戳（高峰前 5 小时）
                                        let trigger_ts = peak_ts - 5 * 3600;

                                        // 生成任务 ID
                                        let peak_date =
                                            chrono::DateTime::from_timestamp(peak_ts, 0)
                                                .map(|dt| dt.with_timezone(&Local))
                                                .unwrap_or(now);
                                        let task_id =
                                            peak_date.format("%Y-%m-%d_%H:%M").to_string();

                                        // 检查是否应该触发（当前时间在触发时间的 ±30 秒内）
                                        let should_trigger =
                                            (current_timestamp - trigger_ts).abs() < 30;

                                        if should_trigger {
                                            let mut queue = WARMUP_QUEUE.lock().unwrap();

                                            // 检查任务是否已存在
                                            let exists = queue.iter().any(|t| t.task_id == task_id);

                                            if !exists {
                                                // 检查是否刚触发过（防止重复）
                                                let mut last = LAST_TRIGGER_TIME.lock().unwrap();

                                                if *last != Some(task_id.clone()) {
                                                    *last = Some(task_id.clone());
                                                    drop(last);

                                                    queue.push(WarmupTask {
                                                        trigger_ts,
                                                        peak_ts,
                                                        task_id: task_id.clone(),
                                                        status: WarmupStatus::Pending,
                                                        retry_count: 0,
                                                    });

                                                    let trigger_time =
                                                        chrono::DateTime::from_timestamp(
                                                            trigger_ts, 0,
                                                        )
                                                        .map(|dt| {
                                                            dt.with_timezone(&Local)
                                                                .format("%m-%d %H:%M")
                                                                .to_string()
                                                        })
                                                        .unwrap_or_default();
                                                    let peak_time =
                                                        peak_date.format("%m-%d %H:%M").to_string();

                                                    logger::log_info(&format!(
                                                    "[Scheduler] ➕ Added warmup task: {} (trigger: {}, peak: {})",
                                                    task_id, trigger_time, peak_time
                                                ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        };

                    // 检查今天和明天的触发
                    check_and_add_tasks(&current_weekday, now);
                    check_and_add_tasks(&next_weekday, next_day);

                    // ===== 步骤 2: 清理过期任务 =====
                    {
                        let mut queue = WARMUP_QUEUE.lock().unwrap();
                        let before_len = queue.len();

                        queue.retain(|t| {
                            // 过期条件：当前时间超过高峰时间 30 分钟，且未完成
                            let expired = current_timestamp > t.peak_ts + 1800
                                && t.status != WarmupStatus::Completed;
                            if expired {
                                logger::log_info(&format!(
                                    "[Scheduler] 🗑️ Removing expired task: {} (peak time + 30min passed)",
                                    t.task_id
                                ));
                            }
                            !expired
                        });

                        // 清理已完成的任务（完成后 30 分钟）
                        queue.retain(|t| {
                            if t.status == WarmupStatus::Completed {
                                current_timestamp < t.peak_ts + 1800
                            } else {
                                true
                            }
                        });

                        if queue.len() != before_len {
                            logger::log_info(&format!(
                                "[Scheduler] 📋 Queue size: {} -> {}",
                                before_len,
                                queue.len()
                            ));
                        }
                    }

                    // ===== 步骤 3: 智能轮询处理队列中的任务 =====
                    let cooldown_ended = {
                        let last_success = LAST_WARMUP_SUCCESS.lock().unwrap();
                        if let Some(ts) = *last_success {
                            // 冷却周期：5 小时 = 18000 秒
                            current_timestamp >= ts + 18000
                        } else {
                            true // 从未成功过，可以尝试
                        }
                    };

                    let tasks_to_process: Vec<WarmupTask> = {
                        let queue = WARMUP_QUEUE.lock().unwrap();
                        queue
                            .iter()
                            .filter(|t| {
                                // 基本条件：未完成、在时间窗口内（触发时间后，高峰时间+30分钟前）
                                let in_window = t.status != WarmupStatus::Completed
                                    && current_timestamp >= t.trigger_ts
                                    && current_timestamp < t.peak_ts + 1800; // 高峰后 30 分钟仍可预热

                                if !in_window {
                                    return false;
                                }

                                // 根据状态决定是否处理
                                match t.status {
                                    WarmupStatus::Pending => true,
                                    WarmupStatus::WaitingCooldown => cooldown_ended,
                                    WarmupStatus::Completed => false,
                                }
                            })
                            .cloned()
                            .collect()
                    };

                    for task in tasks_to_process {
                        let action = if task.status == WarmupStatus::Pending {
                            "Initial execution"
                        } else {
                            "Retry after cooldown"
                        };

                        logger::log_info(&format!(
                            "[Scheduler] 🔥 {} for task: {} (status: {:?}, retry: {})",
                            action, task.task_id, task.status, task.retry_count
                        ));

                        match quota::warm_up_all_accounts().await {
                            Ok(msg) => {
                                logger::log_info(&format!("[Scheduler] Warmup result: {}", msg));

                                let mut queue = WARMUP_QUEUE.lock().unwrap();
                                if let Some(t) =
                                    queue.iter_mut().find(|t| t.task_id == task.task_id)
                                {
                                    if msg.contains("已启动智能预热任务") {
                                        t.status = WarmupStatus::Completed;

                                        let mut last_success = LAST_WARMUP_SUCCESS.lock().unwrap();
                                        *last_success = Some(current_timestamp);

                                        logger::log_info(&format!(
                                            "[Scheduler] ✅ Task completed: {}",
                                            t.task_id
                                        ));
                                    } else if msg.contains("冷却周期中") || msg.contains("无需预热")
                                    {
                                        t.status = WarmupStatus::WaitingCooldown;
                                        t.retry_count += 1;

                                        // 计算预计冷却结束时间
                                        let estimated_end = {
                                            let last_success = LAST_WARMUP_SUCCESS.lock().unwrap();
                                            if let Some(ts) = *last_success {
                                                let end_ts = ts + 18000;
                                                chrono::DateTime::from_timestamp(end_ts, 0)
                                                    .map(|dt| {
                                                        dt.with_timezone(&Local)
                                                            .format("%H:%M")
                                                            .to_string()
                                                    })
                                                    .unwrap_or_else(|| "unknown".to_string())
                                            } else {
                                                "unknown".to_string()
                                            }
                                        };

                                        logger::log_info(&format!(
                                            "[Scheduler] ⏳ Task waiting: {} (cooldown ends ~{})",
                                            t.task_id, estimated_end
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                logger::log_error(&format!("[Scheduler] Warmup failed: {}", e));

                                let mut queue = WARMUP_QUEUE.lock().unwrap();
                                if let Some(t) =
                                    queue.iter_mut().find(|t| t.task_id == task.task_id)
                                {
                                    t.retry_count += 1;
                                }
                            }
                        }
                    }

                    // ===== 步骤 4: 定期日志输出队列状态（每 10 分钟）=====
                    if now.minute() % 10 == 0 {
                        let queue = WARMUP_QUEUE.lock().unwrap();
                        if !queue.is_empty() {
                            let status_summary: Vec<String> = queue
                                .iter()
                                .map(|t| format!("{}({:?})", t.task_id, t.status))
                                .collect();
                            logger::log_info(&format!(
                                "[Scheduler] 📊 Queue status: [{}]",
                                status_summary.join(", ")
                            ));
                        }
                    }
                }
            }
        }
    });
}

fn parse_time_str(s: &str) -> Result<i32, ()> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(());
    }
    let h: i32 = parts[0].parse().map_err(|_| ())?;
    let m: i32 = parts[1].parse().map_err(|_| ())?;
    Ok(h * 60 + m)
}
