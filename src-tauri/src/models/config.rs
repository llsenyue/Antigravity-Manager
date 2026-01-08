use crate::proxy::ProxyConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 时间范围
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: String,
    pub end: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for TimeRange {
    fn default() -> Self {
        Self {
            start: "09:00".to_string(),
            end: "12:00".to_string(),
            enabled: true,
        }
    }
}

/// 定时预热配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledWarmupConfig {
    pub enabled: bool,
    #[serde(default)]
    pub schedules: HashMap<String, Vec<TimeRange>>,
}

impl Default for ScheduledWarmupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedules: HashMap::new(),
        }
    }
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub auto_refresh: bool,
    pub refresh_interval: i32, // 分钟
    pub auto_sync: bool,
    pub sync_interval: i32, // 分钟
    pub default_export_path: Option<String>,
    #[serde(default)]
    pub proxy: ProxyConfig,
    pub antigravity_executable: Option<String>, // [NEW] 手动指定的反重力程序路径
    pub antigravity_args: Option<Vec<String>>,  // [NEW] Antigravity 启动参数
    #[serde(default)]
    pub auto_launch: bool, // 开机自动启动
    #[serde(default)]
    pub scheduled_warmup: ScheduledWarmupConfig,
    #[serde(default)]
    pub token_pool: TokenPoolConfig,
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            auto_refresh: false,
            refresh_interval: 15,
            auto_sync: false,
            sync_interval: 5,
            default_export_path: None,
            proxy: ProxyConfig::default(),
            antigravity_executable: None,
            antigravity_args: None,
            auto_launch: false,
            scheduled_warmup: ScheduledWarmupConfig::default(),
            token_pool: TokenPoolConfig::default(),
        }
    }
}

/// TokenPool 配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenPoolConfig {
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool,
    pub server_url: Option<String>,
    #[serde(default = "default_retry_interval")]
    pub retry_interval: u64,
}

fn default_auto_connect() -> bool {
    false
}

fn default_retry_interval() -> u64 {
    10
}

impl Default for TokenPoolConfig {
    fn default() -> Self {
        Self {
            auto_connect: false,
            server_url: Some("ws://127.0.0.1:8046/ws/supplier".to_string()),
            retry_interval: 10,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
