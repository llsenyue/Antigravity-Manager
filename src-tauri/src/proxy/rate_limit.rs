use dashmap::DashMap;
use std::time::{SystemTime, Duration};
use regex::Regex;

/// 限流信息
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// 限流重置时间
    pub reset_time: SystemTime,
    /// 重试间隔(秒)
    #[allow(dead_code)]
    pub retry_after_sec: u64,
    /// 检测时间
    #[allow(dead_code)]
    pub detected_at: SystemTime,
}

/// 限流跟踪器
pub struct RateLimitTracker {
    limits: DashMap<String, RateLimitInfo>,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            limits: DashMap::new(),
        }
    }
    
    /// 获取账号剩余的等待时间(秒)
    pub fn get_remaining_wait(&self, account_id: &str) -> u64 {
        if let Some(info) = self.limits.get(account_id) {
            let now = SystemTime::now();
            if info.reset_time > now {
                return info.reset_time.duration_since(now).unwrap_or(Duration::from_secs(0)).as_secs();
            }
        }
        0
    }
    
    /// 从错误响应解析限流信息
    /// 
    /// # Arguments
    /// * `account_id` - 账号 ID
    /// * `status` - HTTP 状态码
    /// * `retry_after_header` - Retry-After header 值
    /// * `body` - 错误响应 body
    pub fn parse_from_error(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        body: &str,
    ) -> Option<RateLimitInfo> {
        // 支持 429 (限流) 以及 500/503/529 (后端故障软避让)
        if status != 429 && status != 500 && status != 503 && status != 529 {
            return None;
        }
        
        let mut retry_after_sec = None;
        
        // 1. 从 Retry-After header 提取
        if let Some(retry_after) = retry_after_header {
            if let Ok(seconds) = retry_after.parse::<u64>() {
                retry_after_sec = Some(seconds);
            }
        }
        
        // 2. 从错误消息提取 (优先尝试 JSON 解析，再试正则)
        if retry_after_sec.is_none() {
            retry_after_sec = self.parse_retry_time_from_body(body);
        }
        
        // 3. 处理默认值与软避让逻辑
        let retry_sec = match retry_after_sec {
            Some(s) => {
                // 引入 PR #28 的安全缓冲区：最小 2 秒，防止极高频无效重试
                if s < 2 { 2 } else { s }
            },
            None => {
                if status == 429 {
                    tracing::debug!("无法解析 429 限流时间, 使用默认值 60秒");
                    60
                } else {
                    // 对于 5xx 错误，执行“软避让”：默认锁定 20 秒，强制切换账号
                    tracing::warn!("检测到 5xx 错误 ({}), 执行 20s 软避让...", status);
                    20
                }
            }
        };
        
        let info = RateLimitInfo {
            reset_time: SystemTime::now() + Duration::from_secs(retry_sec),
            retry_after_sec: retry_sec,
            detected_at: SystemTime::now(),
        };
        
        // 存储
        self.limits.insert(account_id.to_string(), info.clone());
        
        tracing::warn!(
            "账号 {} [{}] 状态标记生效, 重置延时: {}秒",
            account_id,
            status,
            retry_sec
        );
        
        Some(info)
    }
    
    /// 从错误消息 body 中解析重置时间
    fn parse_retry_time_from_body(&self, body: &str) -> Option<u64> {
        // A. 优先尝试 JSON 精准解析 (借鉴 PR #28)
        let trimmed = body.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                // 1. Google 常见的 quotaResetDelay 格式 (如 "75.5s" 或 "500ms")
                if let Some(delay_str) = json.get("error")
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.get(0))
                    .and_then(|o| o.get("quotaResetDelay"))
                    .and_then(|v| v.as_str()) {
                    
                    if let Ok(re) = Regex::new(r"(\d+(?:\.\d+)?)(ms|s)") {
                        if let Some(caps) = re.captures(delay_str) {
                            let val = caps[1].parse::<f64>().unwrap_or(0.0);
                            let unit = &caps[2];
                            return if unit == "s" {
                                Some(val.ceil() as u64)
                            } else {
                                Some((val / 1000.0).ceil() as u64)
                            };
                        }
                    }
                }
                
                // 2. OpenAI 常见的 retry_after 字段 (数字)
                if let Some(retry) = json.get("error")
                    .and_then(|e| e.get("retry_after"))
                    .and_then(|v| v.as_u64()) {
                    return Some(retry);
                }
            }
        }

        // B. 正则匹配模式 (兜底)
        // 模式 1: "Try again in 2m 30s"
        if let Ok(re) = Regex::new(r"(?i)try again in (\d+)m\s*(\d+)s") {
            if let Some(caps) = re.captures(body) {
                if let (Ok(m), Ok(s)) = (caps[1].parse::<u64>(), caps[2].parse::<u64>()) {
                    return Some(m * 60 + s);
                }
            }
        }
        
        // 模式 2: "Try again in 30s" 或 "backoff for 42s"
        if let Ok(re) = Regex::new(r"(?i)(?:try again in|backoff for|wait)\s*(\d+)s") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }
        
        // 模式 3: "quota will reset in X seconds"
        if let Ok(re) = Regex::new(r"(?i)quota will reset in (\d+) second") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }
        
        // 模式 4: OpenAI 风格的 "Retry after (\d+) seconds"
        if let Ok(re) = Regex::new(r"(?i)retry after (\d+) second") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }

        // 模式 5: 括号形式 "(wait (\d+)s)"
        if let Ok(re) = Regex::new(r"\(wait (\d+)s\)") {
            if let Some(caps) = re.captures(body) {
                if let Ok(s) = caps[1].parse::<u64>() {
                    return Some(s);
                }
            }
        }
        
        None
    }
    
    /// 获取账号的限流信息
    pub fn get(&self, account_id: &str) -> Option<RateLimitInfo> {
        self.limits.get(account_id).map(|r| r.clone())
    }
    
    /// 检查账号是否仍在限流中
    pub fn is_rate_limited(&self, account_id: &str) -> bool {
        if let Some(info) = self.get(account_id) {
            info.reset_time > SystemTime::now()
        } else {
            false
        }
    }
    
    /// 获取距离限流重置还有多少秒
    pub fn get_reset_seconds(&self, account_id: &str) -> Option<u64> {
        if let Some(info) = self.get(account_id) {
            info.reset_time
                .duration_since(SystemTime::now())
                .ok()
                .map(|d| d.as_secs())
        } else {
            None
        }
    }
    
    /// 清除过期的限流记录
    #[allow(dead_code)]
    pub fn cleanup_expired(&self) -> usize {
        let now = SystemTime::now();
        let mut count = 0;
        
        self.limits.retain(|_k, v| {
            if v.reset_time <= now {
                count += 1;
                false
            } else {
                true
            }
        });
        
        if count > 0 {
            tracing::debug!("清除了 {} 个过期的限流记录", count);
        }
        
        count
    }
    
    /// 清除指定账号的限流记录
    #[allow(dead_code)]
    pub fn clear(&self, account_id: &str) -> bool {
        self.limits.remove(account_id).is_some()
    }
    
    /// 清除所有限流记录
    #[allow(dead_code)]
    pub fn clear_all(&self) {
        let count = self.limits.len();
        self.limits.clear();
        tracing::debug!("清除了所有 {} 条限流记录", count);
    }
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_retry_time_minutes_seconds() {
        let tracker = RateLimitTracker::new();
        let body = "Rate limit exceeded. Try again in 2m 30s";
        let time = tracker.parse_retry_time_from_body(body);
        assert_eq!(time, Some(150)); 
    }
    
    #[test]
    fn test_parse_google_json_delay() {
        let tracker = RateLimitTracker::new();
        let body = r#"{
            "error": {
                "details": [
                    { "quotaResetDelay": "42s" }
                ]
            }
        }"#;
        let time = tracker.parse_retry_time_from_body(body);
        assert_eq!(time, Some(42));
    }

    #[test]
    fn test_parse_retry_after_ignore_case() {
        let tracker = RateLimitTracker::new();
        let body = "Quota limit hit. Retry After 99 Seconds";
        let time = tracker.parse_retry_time_from_body(body);
        assert_eq!(time, Some(99));
    }

    #[test]
    fn test_get_remaining_wait() {
        let tracker = RateLimitTracker::new();
        tracker.parse_from_error("acc1", 429, Some("30"), "");
        let wait = tracker.get_remaining_wait("acc1");
        assert!(wait > 25 && wait <= 30);
    }

    #[test]
    fn test_safety_buffer() {
        let tracker = RateLimitTracker::new();
        // 如果 API 返回 1s，我们强制设为 2s
        tracker.parse_from_error("acc1", 429, Some("1"), "");
        let wait = tracker.get_remaining_wait("acc1");
        assert_eq!(wait, 2);
    }
}
