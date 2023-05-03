pub mod v1 {
    use crate::cloudflare::ApiRequest;
    use crate::datastructures::PostData;
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;
    use headers::HeaderMap;
    use log::info;
    use std::str::FromStr;
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
    ) -> impl IntoResponse {
        staff(id, None, api, headers).await
    }

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
        if uuid::Uuid::from_str(&id).is_err() {
            return BAD_REQUEST;
        }
        let api = api.read().await;

        let header_ip = if let Some(ip) = headers
            .get(api.column())
            .map(|v| v.to_str().unwrap_or_default().to_string())
        {
            ip
        } else {
            String::new()
        };

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

pub use current::{get, post};
pub use v1 as current;
