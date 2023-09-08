pub mod v1 {
    use crate::cloudflare::ApiRequest;
    use crate::datastructures::PostData;
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::{Extension, Json};
    use headers::HeaderMap;
    use log::{info, warn};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    const BAD_REQUEST: (StatusCode, &str) = (StatusCode::BAD_REQUEST, "400 Bad request\n");
    const FORBIDDEN: (StatusCode, &str) = (StatusCode::FORBIDDEN, "403 Forbidden\n");
    const SERVICE_UNAVAILABLE: (StatusCode, &str) = (
        StatusCode::SERVICE_UNAVAILABLE,
        "500 Services Unavailable\n",
    );
    const OK: (StatusCode, &str) = (StatusCode::OK, "200 OK\n");

    pub async fn get(
        Path(id): Path<String>,
        headers: HeaderMap,
        State(api): State<Arc<RwLock<ApiRequest>>>,
        Extension(relay_status): Extension<Arc<AtomicBool>>,
    ) -> impl IntoResponse {
        let post_data = if relay_status.load(Ordering::Relaxed) {
            let api = api.read().await;
            headers
                .get(api.column())
                .map(|ip| {
                    ip.to_str()
                        .map_err(|e| warn!("Convert header value error: {:?}", e))
                        .ok()
                })
                .flatten()
                .map(|ip| PostData::new(ip.to_string()))
        } else {
            None
        };

        staff(id, post_data, api, headers).await
    }

    pub async fn get_debug(mut headers: HeaderMap) -> impl IntoResponse {
        let mut map = serde_json::Map::new();
        for header in headers.drain() {
            if let Some(name) = header.0 {
                map.insert(
                    name.to_string(),
                    serde_json::Value::from(header.1.to_str().ok()),
                );
            }
        }
        Json(map)
    }

    // To use this post function
    // Post data { "ip": "114.51.4.19" } to server
    pub async fn post(
        Path(id): Path<String>,
        State(api): State<Arc<RwLock<ApiRequest>>>,
        headers: HeaderMap,
        Json(data): Json<PostData>,
    ) -> impl IntoResponse {
        staff(id, Some(data), api, headers).await
    }

    async fn staff(
        id: String,
        data: Option<PostData>,
        api: Arc<RwLock<ApiRequest>>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        // Check uuid validity
        if uuid::Uuid::from_str(&id).is_err() {
            return BAD_REQUEST;
        }

        // Configure file
        let api = api.read().await;

        // Get header IP (if empty maybe that's post)
        let header_ip = if let Some(ip) = headers
            .get(api.column())
            .map(|v| v.to_str().unwrap_or_default().to_string())
        {
            ip
        } else {
            String::new()
        };

        // Check is ip from post
        let ret = match data {
            None => {
                if header_ip.is_empty() {
                    return FORBIDDEN;
                }
                api.request(&id, header_ip.clone()).await
            }
            Some(ref data) => api.request(&id, data.ip().to_string()).await,
        };

        match ret {
            Ok(ret) => {
                if ret {
                    if !header_ip.is_empty() && data.is_none() {
                        info!("{} IP updated (via {})", id, header_ip);
                    } else {
                        info!("{} IP updated", id);
                    }
                }
                // Check is relay and is success
                if !(api.is_relay() && !ret) {
                    OK
                } else {
                    SERVICE_UNAVAILABLE
                }
            }
            Err(e) => e.into_response(),
        }
    }
}

pub use current::{get, get_debug, post};
pub use v1 as current;
