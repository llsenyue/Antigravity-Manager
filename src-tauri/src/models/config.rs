use crate::proxy::ProxyConfig;
use serde::{Deserialize, Serialize};

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
    pub scheduled_warmup: ScheduledWarmupConfig, // [NEW] 定时预热配置
    #[serde(default)]
    pub quota_protection: QuotaProtectionConfig, // [NEW] 配额保护配置
}

/// 定时预热配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledWarmupConfig {
    /// 是否启用智能预热
    pub enabled: bool,

    /// 预热模式: "immediate" = 100%即预热, "peak_based" = 高峰期前5小时预热
    #[serde(default = "default_warmup_mode")]
    pub warmup_mode: String,

    /// 预热的模型列表
    #[serde(default = "default_warmup_models")]
    pub monitored_models: Vec<String>,

    /// 高峰期时间列表 (格式: "HH:MM")
    /// 预热将在高峰期前 5 小时触发
    #[serde(default = "default_peak_hours")]
    pub peak_hours: Vec<String>,
}

fn default_warmup_mode() -> String {
    "peak_based".to_string()
}

fn default_warmup_models() -> Vec<String> {
    vec![
        "gemini-3-flash".to_string(),
        "claude-sonnet-4-5".to_string(),
        "gemini-3-pro-high".to_string(),
        "gemini-3-pro-image".to_string(),
    ]
}

fn default_peak_hours() -> Vec<String> {
    vec![
        "10:00".to_string(),
        "15:00".to_string(),
        "20:00".to_string(),
    ]
}

impl ScheduledWarmupConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            warmup_mode: default_warmup_mode(),
            monitored_models: default_warmup_models(),
            peak_hours: default_peak_hours(),
        }
    }
}

impl Default for ScheduledWarmupConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// 配额保护配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaProtectionConfig {
    /// 是否启用配额保护
    pub enabled: bool,

    /// 保留配额百分比 (1-99)
    pub threshold_percentage: u32,

    /// 监控的模型列表 (如 gemini-3-flash, gemini-3-pro-high, claude-sonnet-4-5)
    #[serde(default = "default_monitored_models")]
    pub monitored_models: Vec<String>,
}

fn default_monitored_models() -> Vec<String> {
    vec!["claude-sonnet-4-5".to_string()]
}

impl QuotaProtectionConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            threshold_percentage: 10, // 默认保留10%
            monitored_models: default_monitored_models(),
        }
    }
}

impl Default for QuotaProtectionConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            auto_refresh: true,
            refresh_interval: 15,
            auto_sync: false,
            sync_interval: 5,
            default_export_path: None,
            proxy: ProxyConfig::default(),
            antigravity_executable: None,
            antigravity_args: None,
            auto_launch: false,
            scheduled_warmup: ScheduledWarmupConfig::default(),
            quota_protection: QuotaProtectionConfig::default(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
