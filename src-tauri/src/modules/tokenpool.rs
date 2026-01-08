//! TokenPool 远程接入模块
//!
//! 将本地 Antigravity 反代服务接入 TokenPool 中央调度网络，
//! 实现闲置配额共享变现。

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// TokenPool 服务器默认地址
const DEFAULT_SERVER_URL: &str = "ws://127.0.0.1:8046/ws/supplier";

/// TokenPool 客户端状态
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// 模型配额详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuotaDetail {
    pub name: String,                   // 模型名称，如 "gemini-3-flash"
    pub avg_percentage: f32,            // 平均配额百分比
    pub min_percentage: f32,            // 最低配额百分比
    pub account_count: u32,             // 该模型的账号数
    pub earliest_reset: Option<String>, // 最早冷却结束时间
    #[serde(default)]
    pub quotas: Vec<u8>, // 每个账号的配额百分比列表
    #[serde(default)]
    pub resets: Vec<String>, // 每个账号的刷新时间列表
}

/// 配额状态上报（扩展版）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    // 聚合数据（兼容旧协议）
    pub gemini_flash: f32,
    pub gemini_pro: f32,
    pub claude: f32,

    // 扩展数据
    #[serde(default)]
    pub account_count: u32, // 总账号数
    #[serde(default)]
    pub models: Vec<ModelQuotaDetail>, // 详细模型配额
    #[serde(default)]
    pub next_reset_time: Option<String>, // 最早的冷却结束时间
}

/// 发送给服务器的消息
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "heartbeat")]
    Heartbeat { quota: QuotaStatus },
    #[serde(rename = "response")]
    ProxyResponse {
        request_id: String,
        response: serde_json::Value,
    },
    #[serde(rename = "error")]
    Error { request_id: String, error: String },
}

/// 从服务器接收的消息
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "welcome")]
    Welcome { supplier_id: String },
    #[serde(rename = "request")]
    ProxyRequest {
        request_id: String,
        method: String,
        path: String,
        body: serde_json::Value,
    },
    #[serde(rename = "ack")]
    Ack,
}

/// TokenPool 客户端
pub struct TokenPoolClient {
    /// 连接状态
    status: Arc<RwLock<ConnectionStatus>>,
    /// 供应商 ID (连接后分配)
    supplier_id: Arc<RwLock<Option<String>>>,
    /// 发送消息的通道
    tx: Option<mpsc::Sender<ClientMessage>>,
    /// 本地反代地址
    local_proxy_url: String,
    /// 服务器地址
    server_url: String,
    /// 是否启用共享
    enabled: Arc<RwLock<bool>>,
}

impl TokenPoolClient {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            supplier_id: Arc::new(RwLock::new(None)),
            tx: None,
            local_proxy_url: "http://127.0.0.1:8045".to_string(),
            server_url: DEFAULT_SERVER_URL.to_string(),
            enabled: Arc::new(RwLock::new(false)),
        }
    }

    /// 获取当前连接状态
    pub async fn get_status(&self) -> ConnectionStatus {
        self.status.read().await.clone()
    }

    /// 获取供应商 ID
    pub async fn get_supplier_id(&self) -> Option<String> {
        self.supplier_id.read().await.clone()
    }

    /// 是否已启用共享
    pub async fn is_enabled(&self) -> bool {
        *self.enabled.read().await
    }

    /// 设置服务器地址
    pub fn set_server_url(&mut self, url: &str) {
        self.server_url = url.to_string();
    }

    /// 设置本地反代地址
    pub fn set_local_proxy_url(&mut self, url: &str) {
        self.local_proxy_url = url.to_string();
    }

    /// 连接到 TokenPool 服务器
    pub async fn connect(&mut self) -> Result<(), String> {
        tracing::info!("🔌 Connecting to TokenPool server: {}", self.server_url);

        *self.status.write().await = ConnectionStatus::Connecting;

        let (ws_stream, _) = connect_async(&self.server_url).await.map_err(|e| {
            let err = format!("Failed to connect: {}", e);
            tracing::error!("❌ {}", err);
            err
        })?;

        let (mut write, mut read) = ws_stream.split();

        // 创建消息发送通道
        let (tx, mut rx) = mpsc::channel::<ClientMessage>(32);
        self.tx = Some(tx.clone());

        let status = self.status.clone();
        let supplier_id = self.supplier_id.clone();
        let enabled = self.enabled.clone();
        let local_proxy_url = self.local_proxy_url.clone();

        // 启动发送任务
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let text = serde_json::to_string(&msg).unwrap();
                if write.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
        });

        // 启动接收任务
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<ServerMessage>(&text) {
                            Ok(ServerMessage::Welcome { supplier_id: id }) => {
                                tracing::info!("✅ Connected to TokenPool as supplier: {}", id);
                                *supplier_id.write().await = Some(id);
                                *status.write().await = ConnectionStatus::Connected;
                                *enabled.write().await = true;
                            }
                            Ok(ServerMessage::ProxyRequest {
                                request_id,
                                method,
                                path,
                                body,
                            }) => {
                                tracing::info!(
                                    "📨 Received request: {} {} (id: {})",
                                    method,
                                    path,
                                    request_id
                                );

                                // 转发到本地反代
                                let response =
                                    forward_to_local_proxy(&local_proxy_url, &method, &path, body)
                                        .await;

                                // 发送响应
                                let msg = match response {
                                    Ok(resp) => ClientMessage::ProxyResponse {
                                        request_id,
                                        response: resp,
                                    },
                                    Err(e) => ClientMessage::Error {
                                        request_id,
                                        error: e,
                                    },
                                };
                                let _ = tx_clone.send(msg).await;
                            }
                            Ok(ServerMessage::Ack) => {
                                tracing::debug!("💓 Heartbeat acknowledged");
                            }
                            Err(e) => {
                                tracing::warn!("⚠️ Failed to parse server message: {}", e);
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        tracing::info!("👋 Server closed connection");
                        *status.write().await = ConnectionStatus::Disconnected;
                        *enabled.write().await = false;
                        break;
                    }
                    Err(e) => {
                        tracing::error!("❌ WebSocket error: {}", e);
                        *status.write().await = ConnectionStatus::Error(e.to_string());
                        *enabled.write().await = false;
                        break;
                    }
                    _ => {}
                }
            }
        });

        // 启动心跳任务
        let tx_heartbeat = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                // 获取真实配额
                let quota = calculate_aggregated_quota().await;
                if tx_heartbeat
                    .send(ClientMessage::Heartbeat { quota })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&mut self) {
        tracing::info!("🔌 Disconnecting from TokenPool");
        self.tx = None;
        *self.status.write().await = ConnectionStatus::Disconnected;
        *self.enabled.write().await = false;
        *self.supplier_id.write().await = None;
    }

    /// 发送配额更新
    pub async fn send_quota_update(&self, quota: QuotaStatus) -> Result<(), String> {
        if let Some(tx) = &self.tx {
            tx.send(ClientMessage::Heartbeat { quota })
                .await
                .map_err(|e| e.to_string())
        } else {
            Err("Not connected".to_string())
        }
    }
}

/// 转发请求到本地反代
async fn forward_to_local_proxy(
    base_url: &str,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let url = format!("{}{}", base_url, path);

    tracing::info!("📤 Forwarding to local proxy: {} {}", method, url);

    let request = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url).json(&body),
        "PUT" => client.put(&url).json(&body),
        "DELETE" => client.delete(&url),
        _ => return Err(format!("Unsupported method: {}", method)),
    };

    let response = request
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();

    // 先获取响应文本
    let text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    tracing::info!("📥 Local proxy response: {} (len: {})", status, text.len());

    // 尝试解析为 JSON，如果失败则包装为 JSON
    let body = if text.is_empty() {
        serde_json::json!({
            "status": status.as_u16(),
            "message": "Empty response"
        })
    } else {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(json) => json,
            Err(_) => {
                // 非 JSON 响应，包装为 JSON
                serde_json::json!({
                    "status": status.as_u16(),
                    "data": text
                })
            }
        }
    };

    Ok(body)
}

/// 计算聚合配额（从所有账号获取真实配额数据）
async fn calculate_aggregated_quota() -> QuotaStatus {
    // 获取所有账号
    let accounts = match crate::modules::account::list_accounts() {
        Ok(accs) => accs,
        Err(e) => {
            tracing::warn!("Failed to list accounts for quota: {}", e);
            return QuotaStatus {
                gemini_flash: 0.0,
                gemini_pro: 0.0,
                claude: 0.0,
                account_count: 0,
                models: Vec::new(),
                next_reset_time: None,
            };
        }
    };

    if accounts.is_empty() {
        return QuotaStatus {
            gemini_flash: 0.0,
            gemini_pro: 0.0,
            claude: 0.0,
            account_count: 0,
            models: Vec::new(),
            next_reset_time: None,
        };
    }

    // 统计结构
    struct ModelStats {
        sum: f32,
        min: f32,
        count: u32,
        earliest_reset: Option<String>,
        quotas: Vec<u8>,
        resets: Vec<String>,
    }

    impl ModelStats {
        fn new() -> Self {
            Self {
                sum: 0.0,
                min: f32::MAX,
                count: 0,
                earliest_reset: None,
                quotas: Vec::new(),
                resets: Vec::new(),
            }
        }

        fn add(&mut self, pct: f32, reset_time: Option<&str>) {
            self.sum += pct;
            self.min = self.min.min(pct);
            self.count += 1;
            self.quotas.push(pct as u8); // 收集每个账号的配额
                                         // 收集每个账号的重置时间
            self.resets.push(reset_time.unwrap_or("").to_string());
            // 记录最早的冷却时间
            if let Some(reset) = reset_time {
                if !reset.is_empty() {
                    if self.earliest_reset.is_none()
                        || reset < self.earliest_reset.as_ref().unwrap().as_str()
                    {
                        self.earliest_reset = Some(reset.to_string());
                    }
                }
            }
        }

        fn avg(&self) -> f32 {
            if self.count > 0 {
                self.sum / self.count as f32
            } else {
                0.0
            }
        }

        fn min_val(&self) -> f32 {
            if self.count > 0 {
                self.min
            } else {
                0.0
            }
        }
    }

    // 使用 HashMap 统计所有模型
    use std::collections::HashMap;
    let mut model_stats: HashMap<String, ModelStats> = HashMap::new();
    let account_count = accounts.len() as u32;

    // 同时保留聚合统计用于 legacy 字段
    let mut flash_stats = ModelStats::new();
    let mut pro_stats = ModelStats::new();
    let mut claude_stats = ModelStats::new();

    for account in &accounts {
        if let Some(quota) = &account.quota {
            for model in &quota.models {
                let name = model.name.clone();
                let name_lower = name.to_lowercase();
                let pct = model.percentage as f32;
                let reset = if model.reset_time.is_empty() {
                    None
                } else {
                    Some(model.reset_time.as_str())
                };

                // 按原始模型名称统计
                model_stats
                    .entry(name.clone())
                    .or_insert_with(ModelStats::new)
                    .add(pct, reset);

                // Legacy 聚合统计（用于向后兼容）
                let is_gemini3 = name_lower.contains("gemini-3") || name_lower.contains("gemini_3");
                if is_gemini3 && name_lower.contains("flash") {
                    flash_stats.add(pct, reset);
                } else if is_gemini3 && name_lower.contains("pro") && !name_lower.contains("image")
                {
                    pro_stats.add(pct, reset);
                } else if name_lower.contains("claude") && name_lower.contains("sonnet") {
                    claude_stats.add(pct, reset);
                }
            }
        }
    }

    // 构建完整模型列表（按配额从低到高排序，便于快速发现问题模型）
    let mut models: Vec<ModelQuotaDetail> = model_stats
        .iter()
        .map(|(name, stats)| {
            let mut quotas = stats.quotas.clone();
            quotas.sort(); // 从低到高排序，便于查看
            ModelQuotaDetail {
                name: name.clone(),
                avg_percentage: stats.avg(),
                min_percentage: stats.min_val(),
                account_count: stats.count,
                earliest_reset: stats.earliest_reset.clone(),
                quotas,
                resets: stats.resets.clone(),
            }
        })
        .collect();

    // 按平均配额从低到高排序（配额低的排前面，便于快速发现问题）
    models.sort_by(|a, b| a.avg_percentage.partial_cmp(&b.avg_percentage).unwrap());

    // 找出最早的冷却时间
    let next_reset_time = models
        .iter()
        .filter_map(|m| m.earliest_reset.as_ref())
        .min()
        .cloned();

    tracing::info!(
        "📊 Quota: {} accounts, {} models | Flash {:.0}% | Pro {:.0}% | Claude {:.0}%",
        account_count,
        models.len(),
        flash_stats.avg(),
        pro_stats.avg(),
        claude_stats.avg()
    );

    QuotaStatus {
        gemini_flash: flash_stats.avg(),
        gemini_pro: pro_stats.avg(),
        claude: claude_stats.avg(),
        account_count,
        models,
        next_reset_time,
    }
}

/// 全局 TokenPool 客户端实例
static TOKENPOOL_CLIENT: once_cell::sync::Lazy<Arc<RwLock<TokenPoolClient>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(TokenPoolClient::new())));

/// 获取全局客户端实例
pub fn get_client() -> Arc<RwLock<TokenPoolClient>> {
    TOKENPOOL_CLIENT.clone()
}

// ============= Tauri Commands =============

/// 连接到 TokenPool
#[tauri::command]
pub async fn tokenpool_connect(server_url: Option<String>) -> Result<String, String> {
    let client = get_client();
    let mut guard = client.write().await;
    if let Some(url) = server_url {
        guard.set_server_url(&url);
    }
    guard.connect().await?;
    Ok("Connected to TokenPool".to_string())
}

/// 断开 TokenPool 连接
#[tauri::command]
pub async fn tokenpool_disconnect() -> Result<String, String> {
    let client = get_client();
    let mut guard = client.write().await;
    guard.disconnect().await;
    Ok("Disconnected from TokenPool".to_string())
}

/// 获取 TokenPool 连接状态
#[tauri::command]
pub async fn tokenpool_status() -> Result<serde_json::Value, String> {
    let client = get_client();
    let guard = client.read().await;
    let status = guard.get_status().await;
    let supplier_id = guard.get_supplier_id().await;
    let enabled = guard.is_enabled().await;

    Ok(serde_json::json!({
        "status": format!("{:?}", status),
        "supplier_id": supplier_id,
        "enabled": enabled,
    }))
}
