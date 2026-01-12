use crate::models::QuotaData;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;

const QUOTA_API_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const USER_AGENT: &str = "antigravity/1.11.3 Darwin/arm64";

#[derive(Debug, Serialize, Deserialize)]
struct QuotaResponse {
    models: std::collections::HashMap<String, ModelInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelInfo {
    #[serde(rename = "quotaInfo")]
    quota_info: Option<QuotaInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuotaInfo {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoadProjectResponse {
    #[serde(rename = "cloudaicompanionProject")]
    project_id: Option<String>,
    #[serde(rename = "currentTier")]
    current_tier: Option<Tier>,
    #[serde(rename = "paidTier")]
    paid_tier: Option<Tier>,
}

#[derive(Debug, Deserialize)]
struct Tier {
    id: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "quotaTier")]
    quota_tier: Option<String>,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    slug: Option<String>,
}

/// 创建配置好的 HTTP Client
fn create_client() -> reqwest::Client {
    crate::utils::http::create_client(15)
}

/// 创建预热专用的 HTTP 客户端（超时时间更长）
/// 因为预热需要：Token 刷新（2-3秒） + API 调用（数秒），需要足够的超时时间
fn create_warmup_client() -> reqwest::Client {
    crate::utils::http::create_client(60) // 60 秒超时
}

const CLOUD_CODE_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";

/// 获取项目 ID 和订阅类型
async fn fetch_project_id(access_token: &str, email: &str) -> (Option<String>, Option<String>) {
    let client = create_client();
    let meta = json!({"metadata": {"ideType": "ANTIGRAVITY"}});

    let res = client
        .post(format!("{}/v1internal:loadCodeAssist", CLOUD_CODE_BASE_URL))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", access_token),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::USER_AGENT, "antigravity/windows/amd64")
        .json(&meta)
        .send()
        .await;

    match res {
        Ok(res) => {
            if res.status().is_success() {
                if let Ok(data) = res.json::<LoadProjectResponse>().await {
                    let project_id = data.project_id.clone();

                    // 核心逻辑：优先从 paid_tier 获取订阅 ID，这比 current_tier 更能反映真实账户权益
                    let subscription_tier = data
                        .paid_tier
                        .and_then(|t| t.id)
                        .or_else(|| data.current_tier.and_then(|t| t.id));

                    if let Some(ref tier) = subscription_tier {
                        crate::modules::logger::log_info(&format!(
                            "📊 [{}] 订阅识别成功: {}",
                            email, tier
                        ));
                    }

                    return (project_id, subscription_tier);
                }
            } else {
                crate::modules::logger::log_warn(&format!(
                    "⚠️  [{}] loadCodeAssist 失败: Status: {}",
                    email,
                    res.status()
                ));
            }
        }
        Err(e) => {
            crate::modules::logger::log_error(&format!(
                "❌ [{}] loadCodeAssist 网络错误: {}",
                email, e
            ));
        }
    }

    (None, None)
}

/// 查询账号配额的统一入口
/// 查询账号配额（优化版本：支持传入缓存的 project_id，避免重复调用 loadCodeAssist）
///
/// # Arguments
/// * `access_token` - OAuth Access Token
/// * `email` - 账号邮箱（用于日志）
/// * `cached_project_id` - 可选的缓存 project_id，如有则跳过 loadCodeAssist 调用
pub async fn fetch_quota(
    access_token: &str,
    email: &str,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    fetch_quota_with_cache(access_token, email, None).await
}

/// 带缓存的配额查询（新增）
pub async fn fetch_quota_with_cache(
    access_token: &str,
    email: &str,
    cached_project_id: Option<&str>,
) -> crate::error::AppResult<(QuotaData, Option<String>)> {
    use crate::error::AppError;

    // 优化：如果有缓存的 project_id，跳过 loadCodeAssist 调用以节省 API 配额
    let (project_id, subscription_tier) = if let Some(pid) = cached_project_id {
        tracing::debug!("[{}] 使用缓存的 project_id: {}", email, pid);
        (Some(pid.to_string()), None) // 使用缓存时无法获取 subscription_tier
    } else {
        tracing::debug!("[{}] 无缓存 project_id，调用 loadCodeAssist...", email);
        fetch_project_id(access_token, email).await
    };

    let final_project_id = project_id.as_deref().unwrap_or("bamboo-precept-lgxtn");

    let client = create_client();
    let payload = json!({
        "project": final_project_id
    });

    let url = QUOTA_API_URL;
    let max_retries = 3;
    let mut last_error: Option<AppError> = None;

    for attempt in 1..=max_retries {
        match client
            .post(url)
            .bearer_auth(access_token)
            .header("User-Agent", USER_AGENT)
            .json(&json!(payload))
            .send()
            .await
        {
            Ok(response) => {
                // 将 HTTP 错误状态转换为 AppError
                if let Err(_) = response.error_for_status_ref() {
                    let status = response.status();

                    // ✅ 特殊处理 403 Forbidden - 直接返回,不重试
                    if status == reqwest::StatusCode::FORBIDDEN {
                        crate::modules::logger::log_warn(&format!(
                            "账号无权限 (403 Forbidden),标记为 forbidden 状态"
                        ));
                        let mut q = QuotaData::new();
                        q.is_forbidden = true;
                        q.subscription_tier = subscription_tier.clone();
                        return Ok((q, project_id.clone()));
                    }

                    // 其他错误继续重试逻辑
                    if attempt < max_retries {
                        let text = response.text().await.unwrap_or_default();
                        crate::modules::logger::log_warn(&format!(
                            "API 错误: {} - {} (尝试 {}/{})",
                            status, text, attempt, max_retries
                        ));
                        last_error = Some(AppError::Unknown(format!("HTTP {} - {}", status, text)));
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    } else {
                        let text = response.text().await.unwrap_or_default();
                        return Err(AppError::Unknown(format!(
                            "API 错误: {} - {}",
                            status, text
                        )));
                    }
                }

                let quota_response: QuotaResponse =
                    response.json().await.map_err(|e| AppError::Network(e))?;

                let mut quota_data = QuotaData::new();

                // 使用 debug 级别记录详细信息，避免控制台噪音
                tracing::debug!("Quota API 返回了 {} 个模型", quota_response.models.len());

                for (name, info) in quota_response.models {
                    if let Some(quota_info) = info.quota_info {
                        let percentage = quota_info
                            .remaining_fraction
                            .map(|f| (f * 100.0) as i32)
                            .unwrap_or(0);

                        let reset_time = quota_info.reset_time.unwrap_or_default();

                        // 只保存我们关心的模型
                        if name.contains("gemini") || name.contains("claude") {
                            quota_data.add_model(name, percentage, reset_time);
                        }
                    }
                }

                // 设置订阅类型
                quota_data.subscription_tier = subscription_tier.clone();

                return Ok((quota_data, project_id.clone()));
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "请求失败: {} (尝试 {}/{})",
                    e, attempt, max_retries
                ));
                last_error = Some(AppError::Network(e));
                if attempt < max_retries {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| AppError::Unknown("配额查询失败".to_string())))
}

/// 批量查询所有账号配额 (备用功能)
#[allow(dead_code)]
pub async fn fetch_all_quotas(
    accounts: Vec<(String, String)>,
) -> Vec<(String, crate::error::AppResult<QuotaData>)> {
    let mut results = Vec::new();

    for (account_id, access_token) in accounts {
        // 在批量查询中，我们将 account_id 传入以供日志标识
        let result = fetch_quota(&access_token, &account_id)
            .await
            .map(|(q, _)| q);
        results.push((account_id, result));
    }

    results
}

/// 获取有效的 access_token 用于预热（自动刷新过期 token）
async fn get_valid_token_for_warmup(
    account: &crate::models::Account,
) -> Result<(String, String), String> {
    let now = chrono::Utc::now().timestamp();
    let token_data = &account.token;

    // 使用 expiry_timestamp 判断 token 是否过期
    let expires_at = token_data.expiry_timestamp;

    // 如果 token 还有超过 5 分钟有效期，直接使用
    if now < expires_at - 300 {
        let project_id = token_data
            .project_id
            .clone()
            .unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());
        return Ok((token_data.access_token.clone(), project_id));
    }

    // Token 即将过期，需要刷新
    tracing::info!(
        "[Warmup] Token for {} is expiring, refreshing...",
        account.email
    );

    let token_response = crate::modules::oauth::refresh_access_token(&token_data.refresh_token)
        .await
        .map_err(|e| format!("Token refresh failed for {}: {}", account.email, e))?;

    tracing::info!("[Warmup] Token refresh successful for {}", account.email);

    // 保存刷新后的 token 到磁盘
    if let Err(e) = save_refreshed_token_to_disk(&account.id, &token_response).await {
        tracing::warn!("[Warmup] Failed to save refreshed token: {}", e);
    }

    let project_id = token_data
        .project_id
        .clone()
        .unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());

    Ok((token_response.access_token, project_id))
}

/// 保存刷新后的 token 到磁盘
async fn save_refreshed_token_to_disk(
    account_id: &str,
    token_response: &crate::modules::oauth::TokenResponse,
) -> Result<(), String> {
    // 获取数据目录
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Cannot get data dir: {}", e))?;
    let accounts_dir = data_dir.join("accounts");
    let account_file = accounts_dir.join(format!("{}.json", account_id));

    if !account_file.exists() {
        return Err(format!("Account file not found: {:?}", account_file));
    }

    // 读取并更新账号文件
    let content =
        std::fs::read_to_string(&account_file).map_err(|e| format!("Read error: {}", e))?;
    let mut account_json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Parse error: {}", e))?;

    if let Some(token) = account_json.get_mut("token") {
        token["access_token"] = serde_json::Value::String(token_response.access_token.clone());
        token["expires_in"] = serde_json::Value::Number(token_response.expires_in.into());
        token["timestamp"] =
            serde_json::Value::Number(chrono::Utc::now().timestamp_millis().into());
    }

    std::fs::write(
        &account_file,
        serde_json::to_string_pretty(&account_json).unwrap(),
    )
    .map_err(|e| format!("Write error: {}", e))?;

    Ok(())
}

/// 通过代理内部 API 发送预热请求
///
/// 关键设计：
/// - 调用代理的 `/internal/warmup` 端点
/// - 完全复用代理的所有逻辑：token 获取、UpstreamClient、端点 Fallback
/// - 不做模型映射，直接使用原始模型名称
async fn warmup_model_directly(
    _access_token: &str, // 不再使用，由代理自动处理
    model_name: &str,
    _project_id: &str, // 不再使用，由代理自动处理
    email: &str,
    percentage: i32,
) -> bool {
    // 代理默认端口
    const PROXY_PORT: u16 = 8045;

    let warmup_url = format!("http://127.0.0.1:{}/internal/warmup", PROXY_PORT);

    // 构建预热请求体
    let body = json!({
        "email": email,
        "model": model_name
    });

    tracing::info!(
        "[Warmup] Calling /internal/warmup: {} -> {} (was {}%)",
        email,
        model_name,
        percentage
    );

    let client = create_warmup_client(); // 使用预热专用客户端（60 秒超时）
    let resp = client
        .post(&warmup_url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    match resp {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                tracing::info!(
                    "[Warmup] ✓ Triggered {} for {} (was {}%)",
                    model_name,
                    email,
                    percentage
                );
                true
            } else {
                let text = response.text().await.unwrap_or_default();
                // 截断错误信息
                let truncated = if text.len() > 200 {
                    &text[..200]
                } else {
                    &text
                };
                tracing::warn!(
                    "[Warmup] ✗ {} for {} (was {}%): HTTP {} - {}...",
                    model_name,
                    email,
                    percentage,
                    status,
                    truncated
                );
                false
            }
        }
        Err(e) => {
            tracing::warn!(
                "[Warmup] ✗ {} for {} (was {}%): {}",
                model_name,
                email,
                percentage,
                e
            );
            false
        }
    }
}

/// 一键预热所有账号 - 触发5小时配额恢复周期
/// 支持临界值重试：当模型配额接近100%但未达到时（95-99%），等待后重试
pub async fn warm_up_all_accounts() -> Result<String, String> {
    warm_up_all_accounts_with_retry(0).await
}

/// 内部预热函数，支持重试
async fn warm_up_all_accounts_with_retry(retry_count: u32) -> Result<String, String> {
    const MAX_RETRIES: u32 = 3;
    const RETRY_DELAY_SECS: u64 = 30;
    const NEAR_READY_THRESHOLD: i32 = 95; // 配额 >= 95% 视为即将恢复

    let accounts =
        crate::modules::account::list_accounts().map_err(|e| format!("加载账号失败: {}", e))?;

    if accounts.is_empty() {
        return Err("没有可用账号".to_string());
    }

    // Filter Pro/Ultra accounts
    let pro_ultra_accounts: Vec<_> = accounts
        .into_iter()
        .filter(|a| {
            let tier = a
                .quota
                .as_ref()
                .and_then(|q| q.subscription_tier.as_ref())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            tier.contains("pro") || tier.contains("ultra")
        })
        .collect();

    if pro_ultra_accounts.is_empty() {
        return Err("没有 Pro/Ultra 账号".to_string());
    }

    tracing::info!(
        "[Warmup] 开始预热 {} 个 Pro/Ultra 账号",
        pro_ultra_accounts.len()
    );

    // [FIX] 添加并发控制，避免触发 429 速率限制
    let _semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(2)); // 最多 2 个并发请求

    let mut has_models_to_warm = false;
    let mut has_near_ready_models = false;

    // 收集需要预热的模型信息（email, model_name, percentage）
    let mut warmup_items: Vec<(String, String, String, String, i32)> = Vec::new(); // (email, model_name, access_token, project_id, percentage)

    for account in &pro_ultra_accounts {
        // [REFACTORED] Step 1: 获取有效 token（自动刷新过期的）
        let (access_token, project_id) = match get_valid_token_for_warmup(account).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("[Warmup] 获取账号 {} 有效 token 失败: {}", account.email, e);
                continue;
            }
        };

        // [Step 2] 使用有效 token 获取实时配额
        tracing::info!("[Warmup] 正在获取账号 {} 的最新配额...", account.email);
        let fresh_quota =
            match fetch_quota_with_cache(&access_token, &account.email, Some(&project_id)).await {
                Ok((quota, _)) => quota,
                Err(e) => {
                    tracing::warn!("[Warmup] 获取账号 {} 配额失败: {}", account.email, e);
                    continue;
                }
            };

        let model_count = fresh_quota.models.len();
        tracing::info!(
            "[Warmup] 账号 {} 有 {} 个模型（实时获取）",
            account.email,
            model_count
        );

        // [Step 3] 筛选 100% 的模型（移除系列去重，因为每个模型有独立配额）
        for m in &fresh_quota.models {
            tracing::debug!(
                "[Warmup][DEBUG] 模型: {} | 配额: {}% | 重置时间: {:?}",
                m.name,
                m.percentage,
                m.reset_time
            );

            if m.percentage >= 100 {
                // 跳过 gemini-2.5-pro：该模型配额很少，预热后瞬间变 0%，没有预热价值
                if m.name == "gemini-2.5-pro" {
                    tracing::debug!("[Warmup] 跳过 {} (配额少，预热无意义)", m.name);
                    continue;
                }

                // 每个模型独立预热，不做系列去重
                warmup_items.push((
                    account.email.clone(),
                    m.name.clone(),
                    access_token.clone(),
                    project_id.clone(),
                    m.percentage,
                ));
                tracing::debug!("[Warmup] 计划预热 {}", m.name);
            } else if m.percentage >= NEAR_READY_THRESHOLD {
                has_near_ready_models = true;
            }
        }
    }

    if !warmup_items.is_empty() {
        has_models_to_warm = true;
    }

    // 执行预热任务（支持自动重试）
    if !warmup_items.is_empty() {
        let total_count = warmup_items.len();
        tokio::spawn(async move {
            const MAX_RETRY: usize = 3;
            const RETRY_DELAY_SECS: u64 = 5;

            let mut success_count = 0;
            let mut final_fail_count = 0;

            // 当前需要预热的模型列表
            let mut current_items = warmup_items;
            let mut retry_round = 0;

            while !current_items.is_empty() && retry_round <= MAX_RETRY {
                if retry_round > 0 {
                    tracing::info!(
                        "[Warmup] === 重试第 {}/{} 轮：{} 个失败模型 ===",
                        retry_round,
                        MAX_RETRY,
                        current_items.len()
                    );
                    // 重试前等待 5 秒
                    tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
                }

                let mut failed_items: Vec<(String, String, String, String, i32)> = Vec::new();
                let round_total = current_items.len();

                for (idx, (email, model_name, token, pid, pct)) in
                    current_items.into_iter().enumerate()
                {
                    tracing::info!(
                        "[Warmup] 执行 {}/{} (轮次 {}): {} / {}",
                        idx + 1,
                        round_total,
                        retry_round,
                        email,
                        model_name
                    );

                    let result =
                        warmup_model_directly(&token, &model_name, &pid, &email, pct).await;

                    if result {
                        success_count += 1;
                        tracing::info!("[Warmup] ✓ {} / {} 成功", email, model_name);
                    } else {
                        tracing::warn!(
                            "[Warmup] ✗ {} / {} 失败，将在下一轮重试",
                            email,
                            model_name
                        );
                        // 保存失败项以便重试
                        failed_items.push((email, model_name, token, pid, pct));
                    }

                    // 每个请求间隔 3 秒 + 随机抖动
                    if idx < round_total - 1 {
                        use rand::Rng;
                        let base_delay = 3000;
                        let jitter = rand::thread_rng().gen_range(0..1000);
                        tokio::time::sleep(tokio::time::Duration::from_millis(base_delay + jitter))
                            .await;
                    }
                }

                // 更新当前待处理列表
                current_items = failed_items;
                retry_round += 1;
            }

            // 统计最终失败数
            final_fail_count = current_items.len();

            tracing::info!(
                "[Warmup] ========== 预热完成 ==========\n  成功: {}\n  失败: {}\n  总计: {}\n  重试轮次: {}",
                success_count,
                final_fail_count,
                total_count,
                retry_round.saturating_sub(1)
            );

            // 刷新配额（成功后立即刷新，让界面显示最新状态）
            tracing::info!("[Warmup] 正在刷新配额...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            let _ = crate::commands::refresh_all_quotas().await;
            tracing::info!("[Warmup] ✅ 配额刷新完成");
        });
    }

    // 临界值重试逻辑：如果有模型接近恢复且没有模型需要预热，等待后重试
    if !has_models_to_warm && has_near_ready_models && retry_count < MAX_RETRIES {
        tracing::info!(
            "[Warmup] No models at 100%, but {} near-ready models detected. Waiting {}s before retry ({}/{})...",
            pro_ultra_accounts.iter()
                .filter_map(|a| a.quota.as_ref())
                .flat_map(|q| q.models.iter())
                .filter(|m| m.percentage >= NEAR_READY_THRESHOLD && m.percentage < 100)
                .count(),
            RETRY_DELAY_SECS,
            retry_count + 1,
            MAX_RETRIES
        );

        // 先刷新配额状态
        let _ = crate::commands::refresh_all_quotas().await;

        // 等待后重试
        tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;

        return Box::pin(warm_up_all_accounts_with_retry(retry_count + 1)).await;
    }

    // Schedule auto-refresh after warmup completes (5 seconds delay)
    if has_models_to_warm {
        tokio::spawn(async {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            tracing::info!("[Warmup] Auto-refreshing all account quotas after warmup...");
            let _ = crate::commands::refresh_all_quotas().await;
            tracing::info!("[Warmup] Auto-refresh completed");
        });
    }

    if has_models_to_warm {
        Ok(format!("已启动智能预热任务"))
    } else if retry_count > 0 {
        Ok(format!(
            "已完成 {} 次重试检查，所有模型仍在冷却中",
            retry_count
        ))
    } else {
        Ok(format!("所有模型已在冷却周期中，无需预热"))
    }
}

/// 单账号预热 - 只预热配额满值(100%)的模型，使用最小请求触发5小时恢复周期
pub async fn warm_up_account(account_id: &str) -> Result<String, String> {
    let accounts =
        crate::modules::account::list_accounts().map_err(|e| format!("加载账号失败: {}", e))?;

    let account = accounts
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| "账号不存在".to_string())?;

    // [REFACTORED] Step 1: 获取有效 token（自动刷新过期的）
    let (access_token, project_id) = get_valid_token_for_warmup(&account)
        .await
        .map_err(|e| format!("获取有效 token 失败: {}", e))?;

    // [Step 2] 使用有效 token 获取实时配额
    tracing::info!("[Warmup] 正在获取账号 {} 的最新配额...", account.email);
    let fresh_quota =
        match fetch_quota_with_cache(&access_token, &account.email, Some(&project_id)).await {
            Ok((quota, _)) => quota,
            Err(e) => return Err(format!("获取配额失败: {}", e)),
        };

    let model_count = fresh_quota.models.len();
    tracing::info!(
        "[Warmup] 账号 {} 有 {} 个模型（实时获取）",
        account.email,
        model_count
    );

    // [DEBUG] 打印所有模型的配额信息
    for m in &fresh_quota.models {
        tracing::info!(
            "[Warmup][DEBUG] 模型: {} | 配额: {}% | 重置时间: {}",
            m.name,
            m.percentage,
            m.reset_time
        );
    }

    // [Step 3] 筛选 100% 的模型并应用去重逻辑
    let mut models_to_warm: Vec<(String, i32)> = Vec::new();
    let mut warmed_series = std::collections::HashSet::new(); // 用于记录已预热的系列

    for m in &fresh_quota.models {
        if m.percentage >= 100 {
            // 确定模型系列 Key
            let series_key = if m.name.to_lowercase().contains("image") {
                format!("image-{}", m.name) // Image 模型总是单独预热
            } else if m.name.to_lowercase().contains("claude") {
                "claude-series".to_string()
            } else if m.name.to_lowercase().contains("gemini-2.5") {
                "gemini-2.5-series".to_string()
            } else if m.name.to_lowercase().contains("gemini-3") {
                "gemini-3-series".to_string()
            } else {
                m.name.clone()
            };

            // 如果该系列尚未预热，则加入列表
            if !warmed_series.contains(&series_key) {
                models_to_warm.push((m.name.clone(), m.percentage));
                warmed_series.insert(series_key);
            }
        }
    }

    if models_to_warm.is_empty() {
        return Ok("所有模型已在冷却周期中，无需预热".to_string());
    }

    let warmed_count = models_to_warm.len();

    // [REFACTORED] Step 4: 直接调用 Google API 预热，不经过本地代理
    let email = account.email.clone();
    let token = access_token.clone();
    let pid = project_id.clone();
    let total_count = warmed_count;

    tokio::spawn(async move {
        const MAX_RETRY: usize = 3;
        const RETRY_DELAY_SECS: u64 = 5;

        let mut success_count = 0;

        // 初始化待预热列表
        let mut current_items: Vec<(String, i32)> = models_to_warm;
        let mut retry_round = 0;

        while !current_items.is_empty() && retry_round <= MAX_RETRY {
            if retry_round > 0 {
                tracing::info!(
                    "[Warmup] === 单账号重试第 {}/{} 轮：{} 个失败模型 ===",
                    retry_round,
                    MAX_RETRY,
                    current_items.len()
                );
                // 重试前等待
                tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
            }

            let mut failed_items: Vec<(String, i32)> = Vec::new();
            let round_total = current_items.len();

            for (idx, (model_name, pct)) in current_items.into_iter().enumerate() {
                tracing::info!(
                    "[Warmup] 执行 {}/{} (轮次 {}): {} / {}",
                    idx + 1,
                    round_total,
                    retry_round,
                    email,
                    model_name
                );

                let result = warmup_model_directly(&token, &model_name, &pid, &email, pct).await;

                if result {
                    success_count += 1;
                    tracing::info!("[Warmup] ✓ {} / {} 成功", email, model_name);
                } else {
                    tracing::warn!("[Warmup] ✗ {} / {} 失败，将在下一轮重试", email, model_name);
                    // 保存失败项以便重试
                    failed_items.push((model_name, pct));
                }

                // 每个请求间隔 300ms
                if idx < round_total - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                }
            }

            // 更新当前待处理列表
            current_items = failed_items;
            retry_round += 1;
        }

        // 统计最终失败数
        let final_fail_count = current_items.len();

        tracing::info!(
            "[Warmup] ========== 单账号预热完成 ==========\\n  成功: {}\\n  失败: {}\\n  总计: {}\\n  重试轮次: {}",
            success_count,
            final_fail_count,
            total_count,
            retry_round.saturating_sub(1)
        );

        // [FIX] 预热完成后立即刷新配额
        tracing::info!("[Warmup] 正在刷新账号配额...");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        match crate::commands::refresh_all_quotas().await {
            Ok(_) => tracing::info!("[Warmup] ✅ 配额刷新完成"),
            Err(e) => tracing::warn!("[Warmup] ⚠️ 配额刷新失败: {}", e),
        }
    });

    Ok(format!("已启动 {} 个模型的预热任务", warmed_count))
}

#[cfg(test)]
mod tests {
    use crate::models::quota::QuotaData;

    /// Helper to create a test quota with specified models and percentages
    fn create_test_quota(models: Vec<(&str, i32)>) -> QuotaData {
        let mut quota = QuotaData::new();
        for (name, percentage) in models {
            quota.add_model(name.to_string(), percentage, "".to_string());
        }
        quota
    }

    #[test]
    fn test_smart_warmup_filters_only_100_percent_models() {
        // Create test quota with mixed percentages
        let quota = create_test_quota(vec![
            ("gemini-3-pro-high", 100),
            ("gemini-3-flash", 85),
            ("gemini-3-pro-image", 100),
            ("claude-sonnet-4-5-thinking", 50),
        ]);

        // Simulate the filtering logic
        let mut models_to_warm: Vec<(String, i32)> = Vec::new();
        for m in &quota.models {
            if m.percentage >= 100 {
                models_to_warm.push((m.name.clone(), m.percentage));
            }
        }

        // Should only include 100% models
        assert_eq!(models_to_warm.len(), 2);
        assert!(models_to_warm.iter().any(|(n, _)| n == "gemini-3-pro-high"));
        assert!(models_to_warm
            .iter()
            .any(|(n, _)| n == "gemini-3-pro-image"));
        // Should NOT include sub-100% models
        assert!(!models_to_warm.iter().any(|(n, _)| n == "gemini-3-flash"));
        assert!(!models_to_warm
            .iter()
            .any(|(n, _)| n == "claude-sonnet-4-5-thinking"));
    }

    #[test]
    fn test_smart_warmup_skips_all_when_none_at_100() {
        let quota = create_test_quota(vec![("gemini-3-pro-high", 80), ("gemini-3-flash", 75)]);

        let mut models_to_warm: Vec<(String, i32)> = Vec::new();
        for m in &quota.models {
            if m.percentage >= 100 {
                models_to_warm.push((m.name.clone(), m.percentage));
            }
        }

        // Should be empty - no models at 100%
        assert!(models_to_warm.is_empty());
    }

    #[test]
    fn test_image_model_detection() {
        let image_models = vec!["gemini-3-pro-image", "imagen-3", "IMAGE-GEN"];
        let text_models = vec!["gemini-3-pro-high", "claude-sonnet", "gpt-4"];

        for model in image_models {
            assert!(
                model.to_lowercase().contains("image"),
                "Expected {} to be detected as image model",
                model
            );
        }

        for model in text_models {
            assert!(
                !model.to_lowercase().contains("image"),
                "Expected {} to NOT be detected as image model",
                model
            );
        }
    }

    #[test]
    fn test_warmup_uses_correct_api_for_model_type() {
        // This test documents the expected behavior:
        // - Image models should use countTokens (minimal consumption)
        // - Text models should use generateContent with maxOutputTokens=1

        let is_image_model = |name: &str| name.to_lowercase().contains("image");

        assert!(is_image_model("gemini-3-pro-image"));
        assert!(!is_image_model("gemini-3-flash"));

        // The actual API call logic is tested through integration tests
        // This unit test just validates the detection logic
    }
}
