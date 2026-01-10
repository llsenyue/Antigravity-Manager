// 预热处理器 - 内部预热 API
//
// 提供 /internal/warmup 端点，支持：
// - 指定账号（通过 email）
// - 指定模型（不做映射，直接使用原始模型名称）
// - 复用代理的所有基础设施（UpstreamClient、TokenManager）
// - Claude 模型使用 transform_claude_request_in 转换
// - Gemini 模型使用 wrap_request 转换

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::proxy::mappers::gemini::wrapper::wrap_request;
use crate::proxy::server::AppState;

/// 预热请求体
#[derive(Debug, Deserialize)]
pub struct WarmupRequest {
    /// 账号邮箱
    pub email: String,
    /// 模型名称（原始名称，不做映射）
    pub model: String,
}

/// 预热响应
#[derive(Debug, Serialize)]
pub struct WarmupResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 处理预热请求
pub async fn handle_warmup(
    State(state): State<AppState>,
    Json(req): Json<WarmupRequest>,
) -> Response {
    info!(
        "[Warmup-API] ========== START: email={}, model={} ==========",
        req.email, req.model
    );

    // ===== 步骤 1: 获取 Token =====
    info!("[Warmup-API] Step 1: Getting token for {}", req.email);
    let start_token = std::time::Instant::now();

    let (access_token, project_id, _email) =
        match state.token_manager.get_token_by_email(&req.email).await {
            Ok(result) => {
                info!(
                    "[Warmup-API] Step 1 SUCCESS: Got token in {:?}, project_id={}",
                    start_token.elapsed(),
                    result.1
                );
                result
            }
            Err(e) => {
                warn!(
                    "[Warmup-API] Step 1 FAILED: Token error for {}: {}",
                    req.email, e
                );
                return (
                    StatusCode::BAD_REQUEST,
                    Json(WarmupResponse {
                        success: false,
                        message: format!("Failed to get token for {}", req.email),
                        error: Some(e),
                    }),
                )
                    .into_response();
            }
        };

    // ===== 步骤 2: 根据模型类型构建请求体 =====
    let is_claude = req.model.to_lowercase().contains("claude");
    let is_image = req.model.to_lowercase().contains("image");

    info!(
        "[Warmup-API] Step 2: Building request body for model={}, is_claude={}, is_image={}",
        req.model, is_claude, is_image
    );

    let body: Value = if is_claude {
        // Claude 模型：使用 transform_claude_request_in 转换
        info!(
            "[Warmup-API] Step 2: Using Claude transform for {}",
            req.model
        );

        // 构建最简单的 Claude 请求
        let claude_request = crate::proxy::mappers::claude::models::ClaudeRequest {
            model: req.model.clone(),
            messages: vec![crate::proxy::mappers::claude::models::Message {
                role: "user".to_string(),
                content: crate::proxy::mappers::claude::models::MessageContent::String(
                    "ping".to_string(),
                ),
            }],
            max_tokens: Some(1),
            stream: false,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            metadata: None,
            thinking: None,
            output_config: None,
        };

        // 使用 Claude -> Gemini 转换
        match crate::proxy::mappers::claude::transform_claude_request_in(
            &claude_request,
            &project_id,
        ) {
            Ok(transformed) => {
                info!("[Warmup-API] Step 2 COMPLETE: Claude transform successful");
                transformed
            }
            Err(e) => {
                warn!("[Warmup-API] Step 2 FAILED: Claude transform error: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(WarmupResponse {
                        success: false,
                        message: format!("Transform error: {}", e),
                        error: Some(e),
                    }),
                )
                    .into_response();
            }
        }
    } else {
        // Gemini 模型：使用 wrap_request
        info!(
            "[Warmup-API] Step 2: Using Gemini wrap_request for {}",
            req.model
        );

        let base_request = if is_image {
            json!({
                "model": req.model,
                "contents": [{"role": "user", "parts": [{"text": "Say hi"}]}],
                "generationConfig": {
                    "maxOutputTokens": 10,
                    "responseModalities": ["TEXT"]
                }
            })
        } else {
            // 不设置 maxOutputTokens，让 Google 使用默认值
            // 这样更接近正常请求，避免被 429 拒绝
            json!({
                "model": req.model,
                "contents": [{"role": "user", "parts": [{"text": "Say hi"}]}]
            })
        };

        let wrapped = wrap_request(&base_request, &project_id, &req.model);
        info!(
            "[Warmup-API] Step 2 COMPLETE: requestType={}, finalModel={}",
            wrapped
                .get("requestType")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
            wrapped
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        );
        wrapped
    };

    debug!(
        "[Warmup-API] Step 2 BODY: {}",
        serde_json::to_string_pretty(&body).unwrap_or_default()
    );

    // ===== 步骤 3: 调用 UpstreamClient（先尝试流式，失败后回退非流式）=====
    let model_lower = req.model.to_lowercase();
    // 某些模型可能不支持流式请求，需要使用非流式
    let prefer_non_stream = model_lower.contains("flash-lite") || model_lower.contains("2.5-pro");

    let (method, query) = if prefer_non_stream {
        ("generateContent", None)
    } else {
        ("streamGenerateContent", Some("alt=sse"))
    };

    info!(
        "[Warmup-API] Step 3: Calling UpstreamClient.call_v1_internal({}, token_len={}, body_size={})",
        method,
        access_token.len(),
        serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0)
    );

    let start_upstream = std::time::Instant::now();

    let mut result = state
        .upstream
        .call_v1_internal(method, &access_token, body.clone(), query)
        .await;

    // 如果流式请求失败，尝试非流式请求
    if result.is_err() && !prefer_non_stream {
        info!("[Warmup-API] Step 3: Stream request failed, retrying with non-stream...");
        result = state
            .upstream
            .call_v1_internal("generateContent", &access_token, body, None)
            .await;
    }

    let upstream_duration = start_upstream.elapsed();
    info!(
        "[Warmup-API] Step 3 RETURNED in {:?}: is_ok={}",
        upstream_duration,
        result.is_ok()
    );

    // ===== 步骤 4: 处理响应 =====
    info!("[Warmup-API] Step 4: Processing response");

    match result {
        Ok(response) => {
            let status = response.status();
            info!("[Warmup-API] Step 4: Response status={}", status);

            if status.is_success() {
                info!(
                    "[Warmup-API] ========== SUCCESS: {} / {} in {:?} ==========",
                    req.email,
                    req.model,
                    start_token.elapsed()
                );
                (
                    StatusCode::OK,
                    Json(WarmupResponse {
                        success: true,
                        message: format!("Warmup triggered for {}", req.model),
                        error: None,
                    }),
                )
                    .into_response()
            } else {
                let status_code = status.as_u16();
                let error_text = response.text().await.unwrap_or_default();
                let truncated = if error_text.len() > 500 {
                    format!("{}...", &error_text[..500])
                } else {
                    error_text.clone()
                };

                warn!(
                    "[Warmup-API] ========== FAILED: {} / {} - HTTP {} ==========",
                    req.email, req.model, status_code
                );
                warn!("[Warmup-API] Error response body: {}", truncated);

                (
                    StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    Json(WarmupResponse {
                        success: false,
                        message: format!("Warmup failed: HTTP {}", status_code),
                        error: Some(truncated),
                    }),
                )
                    .into_response()
            }
        }
        Err(e) => {
            warn!(
                "[Warmup-API] ========== ERROR: {} / {} - {} ==========",
                req.email, req.model, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WarmupResponse {
                    success: false,
                    message: "Warmup request failed".to_string(),
                    error: Some(e),
                }),
            )
                .into_response()
        }
    }
}
