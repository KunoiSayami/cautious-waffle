pub mod v1 {
    use crate::cloudflare::ApiRequest;
    use crate::datastructures::PostData;
    use crate::IP_COLUMN;
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;
    use headers::HeaderMap;
    use log::info;
    use std::str::FromStr;
    use std::sync::Arc;

    const X_REAL_IP: &'static str = "X-Real-IP";

    const BAD_REQUEST: (StatusCode, &str) = (StatusCode::BAD_REQUEST, "400 Bad request\n");
    const FORBIDDEN: (StatusCode, &str) = (StatusCode::FORBIDDEN, "403 Forbidden\n");
    const OK: (StatusCode, &str) = (StatusCode::OK, "200 OK\n");

    pub async fn get(
        Path(id): Path<String>,
        headers: HeaderMap,
        State(api): State<Arc<ApiRequest>>,
    ) -> impl IntoResponse {
        staff(id, None, api, headers).await
    }

    pub async fn post(
        Path(id): Path<String>,
        State(api): State<Arc<ApiRequest>>,
        headers: HeaderMap,
        Json(data): Json<PostData>,
    ) -> impl IntoResponse {
        staff(id, Some(data), api, headers).await
    }

    async fn staff(
        id: String,
        data: Option<PostData>,
        api: Arc<ApiRequest>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        if uuid::Uuid::from_str(&id).is_err() {
            return BAD_REQUEST;
        }

        let header_ip = if let Some(ip) = headers
            .get(IP_COLUMN.get_or_init(|| X_REAL_IP.to_string()))
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
            Some(data) => api.request(&id, data.ip().to_string()).await,
        };

        match ret {
            Ok(ret) => {
                if ret {
                    if header_ip.is_empty() {
                        info!("{} IP updated", id);
                    } else {
                        info!("{} IP updated (via {})", id, header_ip);
                    }
                }
                OK
            }
            Err(e) => e.into_response(),
        }
    }
}

pub use current::get;
pub use v1 as current;
