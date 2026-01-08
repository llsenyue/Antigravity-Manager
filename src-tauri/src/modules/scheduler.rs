use chrono::{Local, Timelike};
use std::sync::Mutex;
use tokio::time::{self, Duration};

use crate::modules::{config, logger, quota};

// 防止同一分钟内重复触发
static LAST_TRIGGER_TIME: Mutex<Option<String>> = Mutex::new(None);

// 待处理的预热窗口：存储 (触发时间分钟数, 对应的高峰时间分钟数, 日期标识)
// 当预热触发但模型未准备好时，会在这个窗口内持续检查
static PENDING_WARMUP: Mutex<Option<(i32, i32, String)>> = Mutex::new(None);

pub fn start_scheduler() {
    tauri::async_runtime::spawn(async {
        logger::log_info("Smart Scheduler started. Checking for peak usage periods...");
        let mut interval = time::interval(Duration::from_secs(60)); // Check every minute

        loop {
            interval.tick().await;

            let now = Local::now();
            let current_weekday = now.format("%a").to_string().to_lowercase();
            let current_hm = now.format("%H:%M").to_string();
            let current_minutes = (now.hour() * 60 + now.minute()) as i32;
            let today_str = now.format("%Y-%m-%d").to_string();

            let next_day = now.checked_add_signed(chrono::Duration::days(1)).unwrap();
            let next_weekday = next_day.format("%a").to_string().to_lowercase();

            if let Ok(app_config) = config::load_app_config() {
                if app_config.scheduled_warmup.enabled {
                    // 检查是否应该触发新的预热
                    let mut new_trigger: Option<(i32, i32)> = None;

                    let check_schedule = |day_key: &str, is_tomorrow: bool| -> Option<(i32, i32)> {
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

                                if let (Ok(start), Ok(end)) =
                                    (parse_time_str(&range.start), parse_time_str(&range.end))
                                {
                                    let mid = (start + end) / 2;
                                    let trigger_min = mid - 300; // -5 hours

                                    if !is_tomorrow {
                                        if trigger_min >= 0 && trigger_min == current_minutes {
                                            return Some((trigger_min, mid)); // (触发时间, 高峰时间)
                                        }
                                    } else {
                                        if trigger_min < 0 {
                                            let trigger_today_min = 1440 + trigger_min;
                                            if trigger_today_min == current_minutes {
                                                return Some((trigger_today_min, mid + 1440));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None
                    };

                    // 检查今天和明天的触发
                    if let Some(trigger) = check_schedule(&current_weekday, false) {
                        new_trigger = Some(trigger);
                    }
                    if new_trigger.is_none() {
                        if let Some(trigger) = check_schedule(&next_weekday, true) {
                            new_trigger = Some(trigger);
                        }
                    }

                    // 决定是否需要执行预热
                    let should_warmup = {
                        let mut pending = PENDING_WARMUP.lock().unwrap();

                        if let Some(trigger) = new_trigger {
                            // 新触发时间到了，设置待处理窗口
                            let should_run = {
                                let mut last_trigger = LAST_TRIGGER_TIME.lock().unwrap();
                                if *last_trigger != Some(current_hm.clone()) {
                                    *last_trigger = Some(current_hm.clone());
                                    true
                                } else {
                                    false
                                }
                            };

                            if should_run {
                                *pending = Some((trigger.0, trigger.1, today_str.clone()));
                                logger::log_info(&format!(
                                    "[Scheduler] Warmup window opened at {} for peak at {:02}:{:02}",
                                    current_hm, trigger.1 / 60, trigger.1 % 60
                                ));
                                true
                            } else {
                                false
                            }
                        } else if let Some((trigger_min, peak_min, ref date)) = *pending {
                            // 检查是否在待处理窗口内
                            // 窗口范围：从触发时间到高峰时间
                            let in_window = current_minutes >= trigger_min
                                && current_minutes < peak_min
                                && *date == today_str;

                            if in_window {
                                logger::log_info(&format!(
                                    "[Scheduler] Checking pending warmup at {} (window: {:02}:{:02} - {:02}:{:02})",
                                    current_hm,
                                    trigger_min / 60, trigger_min % 60,
                                    peak_min / 60, peak_min % 60
                                ));
                                true
                            } else if current_minutes >= peak_min || *date != today_str {
                                // 超出窗口，清除待处理状态
                                logger::log_info("[Scheduler] Warmup window expired, closing");
                                *pending = None;
                                false
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if should_warmup {
                        match quota::warm_up_all_accounts().await {
                            Ok(msg) => {
                                logger::log_info(&format!("[Scheduler] Warm-up result: {}", msg));

                                // 检查是否成功预热了至少一个模型
                                if msg.contains("已启动智能预热任务") {
                                    // 成功预热，清除待处理状态
                                    let mut pending = PENDING_WARMUP.lock().unwrap();
                                    *pending = None;
                                    logger::log_info(
                                        "[Scheduler] Warmup successful, window closed",
                                    );
                                }
                                // 如果返回 "所有模型已在冷却周期中"，保持窗口继续检查
                            }
                            Err(e) => {
                                logger::log_error(&format!("[Scheduler] Warm-up failed: {}", e));
                            }
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
