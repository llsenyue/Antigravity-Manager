pub mod account;
pub mod config;
pub mod db;
pub mod device;
pub mod i18n;
pub mod logger;
pub mod migration;
pub mod oauth;
pub mod oauth_server;
pub mod process;
pub mod proxy_db;
pub mod quota;
pub mod scheduler;
pub mod tray;
pub mod update_checker;

use crate::models;

// 重新导出常用函数到 modules 命名空间顶级，方便外部调用
pub use account::*;
pub use config::*;
#[allow(unused_imports)]
pub use logger::*;
#[allow(unused_imports)]
pub use quota::*;
// pub use device::*;

pub async fn fetch_quota(
    access_token: &str,
    email: &str,
) -> crate::error::AppResult<(models::QuotaData, Option<String>)> {
    quota::fetch_quota(access_token, email).await
}

/// 带缓存的配额查询（优化版本）
pub async fn fetch_quota_with_cache(
    access_token: &str,
    email: &str,
    cached_project_id: Option<&str>,
) -> crate::error::AppResult<(models::QuotaData, Option<String>)> {
    quota::fetch_quota_with_cache(access_token, email, cached_project_id).await
}
