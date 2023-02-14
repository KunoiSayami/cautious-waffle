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
    use std::sync::Arc;

    #[derive(Clone, Debug)]
    pub struct RealIP(String);

    const X_REAL_IP: &str = "X-REAL-IP";
    static X_REAL_IP_NAME: Lazy<HeaderName> = Lazy::new(|| HeaderName::from_static(X_REAL_IP));

    impl Header for RealIP {
        fn name() -> &'static HeaderName {
            &*X_REAL_IP_NAME
        }

        fn decode<'i, I>(values: &mut I) -> Result<Self, Error>
        where
            Self: Sized,
            I: Iterator<Item = &'i HeaderValue>,
        {
            let value = values.next().ok_or_else(Error::invalid)?;
            let s = value.to_str().map_err(|_| Error::invalid())?;
            if s.parse::<Ipv4Addr>().is_err() {
                Err(Error::invalid())
            } else {
                Ok(RealIP(s.to_string()))
            }
        }

        fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
            let value = HeaderValue::from_str(self.0.as_str())
                .map_err(|e| error!("Encode error: {:?}", e))
                .ok();
            if let Some(v) = value {
                values.extend(std::iter::once(v));
            }
        }
    }

    pub async fn get(
        Path(uuid): Path<String>,
        TypedHeader(header): TypedHeader<RealIP>,
        State(api): State<Arc<ApiRequest>>,
    ) -> impl IntoResponse {
        let ret = api.request(&uuid, header.0).await;
        match ret {
            Ok(ret) => {
                if ret {
                    info!("{} IP updated", uuid);
                }
                (StatusCode::OK, "200 OK")
            }
            Err(e) => e.into_response(),
        }
    }
}

pub use current::get;
pub use v1 as current;
