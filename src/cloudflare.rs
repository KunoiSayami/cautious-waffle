/*
 ** Copyright (C) 2021 KunoiSayami
 **
 ** This file is part of passive-DDNS and is released under
 ** the AGPL v3 License: https://www.gnu.org/licenses/agpl-3.0.txt
 **
 ** This program is free software: you can redistribute it and/or modify
 ** it under the terms of the GNU Affero General Public License as published by
 ** the Free Software Foundation, either version 3 of the License, or
 ** any later version.
 **
 ** This program is distributed in the hope that it will be useful,
 ** but WITHOUT ANY WARRANTY; without even the implied warranty of
 ** MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 ** GNU Affero General Public License for more details.
 **
 ** You should have received a copy of the GNU Affero General Public License
 ** along with this program. If not, see <https://www.gnu.org/licenses/>.
 */
const DEFAULT_TIMEOUT: u64 = 5;
mod api {

    use super::{ApiError, DEFAULT_TIMEOUT};
    use crate::datastructures::{Config, ZoneMapper};
    use anyhow::anyhow;
    use log::{debug, error, warn};
    use serde_derive::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::time::Duration;

    const CLOUDFLARE_API_PREFIX: &str = "https://api.cloudflare.com/client/v4";

    #[derive(Clone, Debug, Deserialize)]
    pub struct DNSRecord {
        id: String,
        zone_id: String,
        name: String,
        content: String,
        proxied: bool,
        ttl: i32,
    }

    impl DNSRecord {
        async fn update_ns_record(&self, session: &reqwest::Client) -> anyhow::Result<bool> {
            let resp = session
                .put(
                    format!(
                        "{}/zones/{}/dns_records/{}",
                        CLOUDFLARE_API_PREFIX, &self.zone_id, &self.id
                    )
                    .as_str(),
                )
                .json(&PutDNSRecord::from(self))
                .send()
                .await
                .map_err(|e| anyhow!("Got error while update DNS record: {:?}", e))?;
            Ok(resp.status().is_success())
        }
        pub fn name(&self) -> &str {
            &self.name
        }
        pub fn content(&self) -> &str {
            &self.content
        }
        pub fn proxied(&self) -> bool {
            self.proxied
        }
        pub fn ttl(&self) -> i32 {
            self.ttl
        }

        pub async fn fetch_dns_record(
            client: &reqwest::Client,
            zone: &str,
            name: &str,
        ) -> anyhow::Result<Self> {
            let resp = client
                .get(format!(
                    "{}/zones/{}/dns_records",
                    CLOUDFLARE_API_PREFIX, zone
                ))
                .query(
                    &[("type", "A"), ("name", name)]
                        .iter()
                        .map(|(x, y)| (x.to_string(), y.to_string()))
                        .collect::<HashMap<String, String>>(),
                )
                .send()
                .await
                .map_err(|e| anyhow!("Got error while query DNS records: {:?}", e))?;
            if !resp.status().is_success() {
                return Err(anyhow!("Api request is unsuccessful: {:?}", resp));
            }
            let resp: CloudFlareResult = resp
                .json()
                .await
                .map_err(|e| anyhow!("Got error while serialize DNS records: {:?}", e))?;
            if !resp.success() {
                return Err(anyhow!(
                    "Got error in cloudflare dns api request: {:?}",
                    resp.errors()
                ));
            }
            serde_json::from_value::<Vec<_>>(resp.result())
                .map_err(|e| anyhow!("Got error while serialize DNS result: {:?}", e))?
                .pop()
                .ok_or(anyhow!("Result is empty!"))
        }
        pub fn set_content(&mut self, content: String) {
            self.content = content;
        }
    }

    #[derive(Clone, Debug, Serialize)]
    struct PutDNSRecord {
        #[serde(rename = "type")]
        type_: String,
        name: String,
        content: String,
        proxied: bool,
        ttl: i32,
    }

    impl From<&DNSRecord> for PutDNSRecord {
        fn from(dns_record: &DNSRecord) -> Self {
            Self {
                type_: 'A'.to_string(),
                name: dns_record.name().to_string(),
                content: dns_record.content().to_string(),
                proxied: dns_record.proxied(),
                ttl: dns_record.ttl(),
            }
        }
    }

    #[allow(dead_code)]
    #[derive(Clone, Debug, Deserialize)]
    pub struct CloudFlareError {
        code: i64,
        message: String,
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct CloudFlareResult {
        success: bool,
        result: serde_json::Value,
        errors: Vec<CloudFlareError>,
    }

    impl CloudFlareResult {
        pub fn success(&self) -> bool {
            self.success
        }
        pub fn result(self) -> serde_json::Value {
            self.result
        }
        pub fn errors(&self) -> &Vec<CloudFlareError> {
            &self.errors
        }
    }

    #[derive(Clone, Debug)]
    pub struct ApiRequest {
        mapper: HashMap<String, Vec<ZoneMapper>>,
        client: reqwest::Client,
    }

    impl From<Config> for ApiRequest {
        fn from(value: Config) -> Self {
            let client = reqwest::ClientBuilder::new()
                .default_headers({
                    let mut m = reqwest::header::HeaderMap::new();
                    m.insert(
                        "Authorization",
                        format!("Bearer {}", value.token()).parse().unwrap(),
                    );
                    m
                })
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT))
                .build()
                .unwrap();
            let mut m = HashMap::new();
            let mut zone_map = HashMap::new();
            for zone in value.zones() {
                zone_map.insert(zone.domain(), zone.zone());
            }
            let mut zones = Vec::new();
            for element in value.clients() {
                for target in element.target() {
                    let target_slice: Vec<_> = target.split('.').collect();
                    for i in 0..target_slice.len() - 1 {
                        let mid = target_slice[i..].join(".");
                        if let Some(zone) = zone_map.get(mid.as_str()) {
                            zones.push(ZoneMapper::new(target.to_string(), zone.to_string()));
                            break;
                        }
                    }
                }
                m.insert(element.uuid().to_string(), zones.clone());
                zones.clear();
            }
            Self { mapper: m, client }
        }
    }

    impl ApiRequest {
        pub async fn request(&self, uuid: &String, new_ip: String) -> Result<bool, ApiError> {
            let zones = self.mapper.get(uuid).ok_or_else(ApiError::forbidden)?;

            if zones.is_empty() {
                warn!("Pending array is empty");
                return Err(ApiError::bad_request());
            }

            for zone in zones {
                if let Ok(mut record) =
                    DNSRecord::fetch_dns_record(&self.client, zone.zone(), zone.domain())
                        .await
                        .map_err(|e| error!("{}", e))
                {
                    if !record.content().eq(&new_ip) {
                        record.set_content(new_ip.to_string());
                        record
                            .update_ns_record(&self.client)
                            .await
                            .map_err(|e| {
                                error!("Processing: {} {} {}", zone.domain(), zone.zone(), e)
                            })
                            .ok();
                    }
                };
            }

            Ok(false)
        }
    }
}

mod api_error {
    use axum::http::StatusCode;
    use log::error;

    #[derive(Debug)]
    pub enum ApiError {
        Forbidden,
        BadRequest,
        Other(anyhow::Error),
    }

    impl ApiError {
        pub fn forbidden() -> Self {
            Self::Forbidden
        }
        pub fn bad_request() -> Self {
            Self::BadRequest
        }

        pub fn into_response(self) -> (StatusCode, &'static str) {
            match self {
                ApiError::Forbidden => (StatusCode::FORBIDDEN, "403 Forbidden"),
                ApiError::Other(e) => {
                    error!("{}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "500 Internal server error",
                    )
                }
                ApiError::BadRequest => (StatusCode::BAD_REQUEST, "400 Bad request"),
            }
        }
    }

    impl From<anyhow::Error> for ApiError {
        fn from(value: anyhow::Error) -> Self {
            Self::Other(value)
        }
    }
}

pub use api::ApiRequest;
pub use api_error::ApiError;
