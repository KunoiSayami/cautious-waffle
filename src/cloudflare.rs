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
const RELAY_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));
mod api {

    use super::{ApiError, DEFAULT_TIMEOUT};
    use crate::cloudflare::RELAY_USER_AGENT;
    use crate::datastructures::{Config, PostData, Relay, RelayConfig, ZoneMapper};
    use anyhow::anyhow;
    use log::{error, info};
    use serde_derive::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::time::Duration;
    use tap::TapFallible;

    const CLOUDFLARE_API_PREFIX: &str = "https://api.cloudflare.com/client/v4";

    pub const DEFAULT_COLUMN: &'static str = "X-Real-IP";

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
        relay: Relay,
        client: reqwest::Client,
        column: String,
    }

    impl TryFrom<RelayConfig> for ApiRequest {
        type Error = anyhow::Error;

        fn try_from(value: RelayConfig) -> Result<Self, Self::Error> {
            let client = reqwest::ClientBuilder::new()
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT))
                .user_agent(RELAY_USER_AGENT);
            let client = if let Some(proxy) = value.proxy() {
                client.proxy(
                    reqwest::Proxy::all(proxy)
                        .map_err(|e| anyhow!("Parse proxy scheme error: {:?}", e))?,
                )
            } else {
                client
            }
            .build()
            .unwrap();
            let relay = Relay::try_from(value)?;
            Ok(Self {
                mapper: HashMap::new(),
                relay,
                client,
                column: "".to_string(),
            })
        }
    }

    impl TryFrom<Config> for ApiRequest {
        type Error = anyhow::Error;

        fn try_from(value: Config) -> Result<Self, Self::Error> {
            let ip_column = value
                .column_ip()
                .clone()
                .unwrap_or_else(|| DEFAULT_COLUMN.to_string());
            if value.is_relay_mode() {
                return Self::try_from(value.relay()).map(|x| x.set_column(ip_column));
            }
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
                if zones.is_empty() {
                    return Err(anyhow!("Zone is empty"));
                }
                m.insert(element.uuid().to_string(), zones.clone());
                zones.clear();
            }
            Ok(Self {
                mapper: m,
                relay: Default::default(),
                client,
                column: ip_column,
            })
        }
    }

    impl ApiRequest {
        pub async fn process_relay(&self, uuid: &String, new_ip: String) -> Result<bool, ApiError> {
            let data = PostData::new(new_ip);
            let mut update = false;
            for upstream in self.relay.target() {
                if let Ok(status) = self
                    .client
                    .post(format!("{}{}", upstream, uuid))
                    .json(&data)
                    .send()
                    .await
                    .map(|ret| ret.status())
                    .tap_err(|e| error!("{}", e))
                {
                    if status.is_success() {
                        update = true;
                        break;
                    }
                    error!("Post to {} unsuccessful: {:?}", upstream, status)
                }
            }
            Ok(update)
        }

        pub async fn request(&self, uuid: &String, new_ip: String) -> Result<bool, ApiError> {
            if self.relay.enabled() {
                let uuid = self
                    .relay
                    .clients()
                    .get(uuid)
                    .ok_or_else(ApiError::forbidden)?;

                return self.process_relay(&uuid, new_ip).await;
            }

            let zones = self.mapper.get(uuid).ok_or_else(ApiError::forbidden)?;

            let mut updated = false;

            for zone in zones {
                if let Ok(mut record) =
                    DNSRecord::fetch_dns_record(&self.client, zone.zone(), zone.domain())
                        .await
                        .tap_err(|e| error!("{}", e))
                {
                    if !record.content().eq(&new_ip) {
                        record.set_content(new_ip.clone());
                        record
                            .update_ns_record(&self.client)
                            .await
                            .map(|ret| {
                                if ret && !updated {
                                    updated = true;
                                    info!("Update {} IP to {}", uuid, new_ip);
                                }
                                ret
                            })
                            .tap_err(|e| {
                                error!("Processing: {} {} {}", zone.domain(), zone.zone(), e)
                            })
                            .ok();
                    }
                };
            }

            Ok(updated)
        }

        pub fn is_relay(&self) -> bool {
            self.relay.enabled()
        }

        pub fn info(&self) -> String {
            format!(
                "relay mode: {}, {}",
                self.is_relay(),
                if self.is_relay() {
                    format!(
                        "targets: {}, clients: {}",
                        self.relay.target().len(),
                        self.relay.clients().len()
                    )
                } else {
                    format!("clients: {}", self.mapper.len())
                }
            )
        }
        fn set_column(mut self, column: String) -> Self {
            self.column = column;
            self
        }
        pub fn column(&self) -> &str {
            &self.column
        }
    }
}

mod api_error {
    use axum::http::StatusCode;
    use log::error;

    #[derive(Debug)]
    pub enum ApiError {
        Forbidden,
        Other(anyhow::Error),
    }

    impl ApiError {
        pub fn forbidden() -> Self {
            Self::Forbidden
        }

        pub fn into_response(self) -> (StatusCode, &'static str) {
            match self {
                ApiError::Forbidden => (StatusCode::FORBIDDEN, "403 Forbidden\n"),
                ApiError::Other(e) => {
                    error!("{}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "500 Internal server error\n",
                    )
                }
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
