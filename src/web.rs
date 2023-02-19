pub mod v1 {
    use crate::cloudflare::ApiRequest;
    use axum::extract::{Path, State};
    use axum::http::header::HeaderName;
    use axum::http::{HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    use axum::TypedHeader;
    use headers::{Error, Header};
    use log::{error, info};
    use once_cell::sync::Lazy;
    use std::net::Ipv4Addr;
    use std::str::FromStr;
    use std::sync::Arc;

    #[derive(Clone, Debug)]
    pub struct RealIP(Option<String>);

    const X_REAL_IP: &'static str = "X-Real-IP";
    static X_REAL_IP_NAME: Lazy<HeaderName> =
        Lazy::new(|| HeaderName::try_from(X_REAL_IP).unwrap());

    impl Header for RealIP {
        fn name() -> &'static HeaderName {
            &*X_REAL_IP_NAME
        }

        fn decode<'i, I>(values: &mut I) -> Result<Self, Error>
        where
            Self: Sized,
            I: Iterator<Item = &'i HeaderValue>,
        {
            Ok(RealIP(
                values
                    .next()
                    .map(|v| {
                        v.to_str()
                            .map(|s| {
                                if s.parse::<Ipv4Addr>().is_err() {
                                    None
                                } else {
                                    Some(s.to_string())
                                }
                            })
                            .ok()
                    })
                    .flatten()
                    .flatten(),
            ))
        }

        fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
            if let Some(ref s) = self.0 {
                let value = HeaderValue::from_str(s.as_str())
                    .map_err(|e| error!("Encode error: {:?}", e))
                    .ok();
                if let Some(v) = value {
                    values.extend(std::iter::once(v));
                }
            }
        }
    }

    pub async fn get(
        Path(id): Path<String>,
        TypedHeader(header): TypedHeader<RealIP>,
        State(api): State<Arc<ApiRequest>>,
    ) -> impl IntoResponse {
        if uuid::Uuid::from_str(&id).is_err() {
            return (StatusCode::BAD_REQUEST, "400 Bad request\n");
        }

        let ret = if let Some(ip) = header.0 {
            api.request(&id, ip).await
        } else {
            return (StatusCode::FORBIDDEN, "403 Forbidden\n");
        };
        match ret {
            Ok(ret) => {
                if ret {
                    info!("{} IP updated", id);
                }
                (StatusCode::OK, "200 OK\n")
            }
            Err(e) => e.into_response(),
        }
    }
}

pub use current::get;
pub use v1 as current;
