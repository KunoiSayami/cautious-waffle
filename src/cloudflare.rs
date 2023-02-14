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
    use crate::datastructures::Config;
    use anyhow::anyhow;
    use log::{error, warn};
    use reqwest::Url;
    use serde_derive::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::time::Duration;

    const CLOUDFLARE_API_PREFIX: &str = "https://api.cloudflare.com/client/v4/";

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
                        "{}zones/{}/dns_records/{}",
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
            let resp: CloudFlareResult = client
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
                .map_err(|e| anyhow!("Got error while query DNS records: {:?}", e))?
                .json()
                .await
                .map_err(|e| anyhow!("Got error while serialize DNS records: {:?}", e))?;
            if !resp.success() {
                return Err(anyhow!(
                    "Got error in cloudflare dns api request: {:?}",
                    resp.errors
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

    #[derive(Deserialize, Clone, Debug)]
    pub struct Zone {
        id: String,
        name: String,
    }

    impl Zone {
        pub fn id(&self) -> &str {
            &self.id
        }
        pub fn name(&self) -> &str {
            &self.name
        }

        pub async fn fetch_zone(client: &reqwest::Client) -> anyhow::Result<Vec<Self>> {
            let ret: CloudFlareResult = client
                .get(format!("{}/zones", CLOUDFLARE_API_PREFIX))
                .query(
                    &[("type".to_string(), "A".to_string())]
                        .iter()
                        .cloned()
                        .collect::<HashMap<String, String>>(),
                )
                .send()
                .await
                .map_err(|e| anyhow!("Got error while request CloudFlare api: {:?}", e))?
                .json()
                .await
                .map_err(|e| anyhow!("Got error while serialize json: {:?}", e))?;
            if !ret.success() {
                return Err(anyhow!("Request api failure: {:?}", ret.errors()));
            }
            serde_json::from_value(ret.result())
                .map_err(|e| anyhow!("Got error while serialize result json: {:?}", e))
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

    #[derive(Clone)]
    pub struct ApiRequest {
        mapper: HashMap<String, Vec<String>>,
        zone_mapper: HashMap<String, String>,
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
            for element in value.clients() {
                m.insert(element.uuid().to_string(), element.target().clone());
            }
            Self {
                mapper: m,
                zone_mapper: Default::default(),
                client,
            }
        }
    }

    impl ApiRequest {
        pub async fn request(&self, uuid: &String, new_ip: String) -> Result<bool, ApiError> {
            if self.zone_mapper.is_empty() {
                warn!("Warning: zone is empty, bypass all request");
                return Err(ApiError::forbidden());
            }
            let domain_names = self.mapper.get(uuid).ok_or_else(ApiError::forbidden)?;
            let mut pending = HashMap::new();
            for name in domain_names {
                if let Some(domain) = Url::parse(name)
                    .map_err(|e| warn!("Unable parse {:?}, skipped: {:?}", name, e))
                    .ok()
                    .map(|url| url.domain().map(|s| s.to_string()))
                    .flatten()
                {
                    if let Some(ret) = self.zone_mapper.get(&domain) {
                        pending.insert(name, ret);
                    } else {
                        warn!("Missing domain: {:?}", domain);
                    }
                }
            }

            if pending.is_empty() {
                warn!("Pending array is empty");
                return Err(ApiError::bad_request());
            }

            //let mut task = JoinSet::new();
            for (name, zone) in &pending {
                //task.spawn(async {
                if let Ok(mut record) = DNSRecord::fetch_dns_record(&self.client, zone, name)
                    .await
                    .map_err(|e| error!("{}", e))
                {
                    if !record.content().eq(&new_ip) {
                        record.set_content(new_ip.to_string());
                        record
                            .update_ns_record(&self.client)
                            .await
                            .map_err(|e| error!("Processing: {} {} {}", name, zone, e))
                            .ok();
                    }
                };
                //});
            }
            //while let Some(_res) = task.join_next().await {}

            Err(ApiError::forbidden())
        }

        pub async fn update_zone_info(mut self) -> anyhow::Result<Self> {
            let zones = Zone::fetch_zone(&self.client).await?;
            self.zone_mapper = {
                let mut m = HashMap::new();
                for zone in zones {
                    m.insert(zone.name().to_string(), zone.id().to_string());
                }
                m
            };
            Ok(self)
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
