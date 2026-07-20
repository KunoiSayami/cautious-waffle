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
    use std::net::IpAddr;
    use std::time::Duration;

    const CLOUDFLARE_API_PREFIX: &str = "https://api.cloudflare.com/client/v4";

    /// TTL for records this server creates; 1 means "automatic" to Cloudflare.
    const DEFAULT_TTL: i32 = 1;

    pub const DEFAULT_COLUMN: &str = "X-Real-IP";

    /// The DNS record type an address maps to: IPv4 to `A`, IPv6 to `AAAA`.
    ///
    /// The variant names are the wire values Cloudflare expects, so they keep
    /// the DNS spelling rather than Rust's usual acronym casing.
    #[allow(clippy::upper_case_acronyms)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
    pub enum RecordType {
        A,
        AAAA,
    }

    impl RecordType {
        pub fn as_str(&self) -> &'static str {
            match self {
                Self::A => "A",
                Self::AAAA => "AAAA",
            }
        }
    }

    impl From<IpAddr> for RecordType {
        fn from(ip: IpAddr) -> Self {
            match ip {
                IpAddr::V4(_) => Self::A,
                IpAddr::V6(_) => Self::AAAA,
            }
        }
    }

    impl std::fmt::Display for RecordType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.as_str())
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct DNSRecord {
        id: String,
        //zone_id: String,
        name: String,
        content: String,
        proxied: bool,
        ttl: i32,
    }

    impl DNSRecord {
        async fn update_ns_record(
            &self,
            zone_id: &str,
            session: &reqwest::Client,
            record_type: RecordType,
        ) -> anyhow::Result<bool> {
            let resp = session
                .put(
                    format!(
                        "{}/zones/{}/dns_records/{}",
                        CLOUDFLARE_API_PREFIX, zone_id, &self.id
                    )
                    .as_str(),
                )
                .json(&PutDNSRecord::from_record(self, record_type))
                .send()
                .await
                .map_err(|e| anyhow!("Got error while update DNS record: {e:?}"))?;
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

        /// Look up the record of `record_type` for `name`.
        ///
        /// Returns `Ok(None)` when the zone simply has no such record yet,
        /// which is the normal case the first time a client publishes an
        /// address of a given family.
        pub async fn fetch_dns_record(
            client: &reqwest::Client,
            zone: &str,
            name: &str,
            record_type: RecordType,
        ) -> anyhow::Result<Option<Self>> {
            let resp = client
                .get(format!("{CLOUDFLARE_API_PREFIX}/zones/{zone}/dns_records",))
                .query(
                    &[("type", record_type.as_str()), ("name", name)]
                        .iter()
                        .map(|(x, y)| (x.to_string(), y.to_string()))
                        .collect::<HashMap<String, String>>(),
                )
                .send()
                .await
                .map_err(|e| anyhow!("Got error while query DNS records: {e:?}"))?;
            if !resp.status().is_success() {
                return Err(anyhow!("Api request is unsuccessful: {resp:?}"));
            }
            let resp: CloudFlareResult = resp
                .json()
                .await
                .map_err(|e| anyhow!("Got error while serialize DNS records: {e:?}"))?;
            //log::debug!("resp => {:?}", resp);
            if !resp.success() {
                return Err(anyhow!(
                    "Got error in cloudflare dns api request: {:?}",
                    resp.errors()
                ));
            }
            Ok(serde_json::from_value::<Vec<_>>(resp.result())
                .map_err(|e| anyhow!("Got error while serialize DNS result: {e:?}"))?
                .pop())
        }

        /// Create a record that does not exist yet.
        async fn create_ns_record(
            client: &reqwest::Client,
            zone_id: &str,
            name: &str,
            content: &str,
            record_type: RecordType,
        ) -> anyhow::Result<bool> {
            let resp = client
                .post(format!(
                    "{CLOUDFLARE_API_PREFIX}/zones/{zone_id}/dns_records"
                ))
                .json(&PutDNSRecord {
                    type_: record_type,
                    name: name.to_string(),
                    content: content.to_string(),
                    proxied: false,
                    ttl: DEFAULT_TTL,
                })
                .send()
                .await
                .map_err(|e| anyhow!("Got error while create DNS record: {e:?}"))?;
            Ok(resp.status().is_success())
        }

        pub fn set_content(&mut self, content: String) {
            self.content = content;
        }
    }

    #[derive(Clone, Debug, Serialize)]
    struct PutDNSRecord {
        #[serde(rename = "type")]
        type_: RecordType,
        name: String,
        content: String,
        proxied: bool,
        ttl: i32,
    }

    impl PutDNSRecord {
        fn from_record(dns_record: &DNSRecord, record_type: RecordType) -> Self {
            Self {
                type_: record_type,
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
                        .map_err(|e| anyhow!("Parse proxy scheme error: {e:?}"))?,
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
                    .inspect_err(|e| error!("{e}"))
                {
                    if status.is_success() {
                        update = true;
                        break;
                    }
                    error!("Post to {upstream} unsuccessful: {status:?}")
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

                return self.process_relay(uuid, new_ip).await;
            }

            // The address family decides which record type to touch, so an
            // IPv6 update leaves the client's A record alone and vice versa.
            let ip: IpAddr = new_ip.parse().map_err(|e| {
                ApiError::bad_request(format!("Invalid IP address {new_ip:?}: {e:?}"))
            })?;
            let record_type = RecordType::from(ip);
            // Normalize so a differently-spelled but equal address (e.g. an
            // abbreviated IPv6 form) is not seen as a change.
            let new_ip = ip.to_string();

            let zones = self.mapper.get(uuid).ok_or_else(ApiError::forbidden)?;

            let mut updated = false;

            for zone in zones {
                let record = match DNSRecord::fetch_dns_record(
                    &self.client,
                    zone.zone(),
                    zone.domain(),
                    record_type,
                )
                .await
                .inspect_err(|e| error!("{e}"))
                {
                    Ok(record) => record,
                    Err(_) => continue,
                };

                let ret = match record {
                    Some(mut record) => {
                        if record.content().eq(&new_ip) {
                            continue;
                        }
                        record.set_content(new_ip.clone());
                        record
                            .update_ns_record(zone.zone(), &self.client, record_type)
                            .await
                    }
                    None => {
                        info!(
                            "Creating {record_type} record for {} in {}",
                            zone.domain(),
                            zone.zone()
                        );
                        DNSRecord::create_ns_record(
                            &self.client,
                            zone.zone(),
                            zone.domain(),
                            &new_ip,
                            record_type,
                        )
                        .await
                    }
                };

                ret.inspect(|ret| {
                    if *ret && !updated {
                        updated = true;
                        info!("Update {uuid} {record_type} record to {new_ip}");
                    }
                })
                .inspect_err(|e| error!("Processing: {} {} {e}", zone.domain(), zone.zone()))
                .ok();
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
        BadRequest(String),
        Other(anyhow::Error),
    }

    impl ApiError {
        pub fn forbidden() -> Self {
            Self::Forbidden
        }

        pub fn bad_request(reason: String) -> Self {
            Self::BadRequest(reason)
        }

        pub fn into_response(self) -> (StatusCode, &'static str) {
            match self {
                ApiError::Forbidden => (StatusCode::FORBIDDEN, "403 Forbidden\n"),
                ApiError::BadRequest(reason) => {
                    error!("{reason}");
                    (StatusCode::BAD_REQUEST, "400 Bad request\n")
                }
                ApiError::Other(e) => {
                    error!("{e}");
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

#[cfg(test)]
mod tests {
    use super::api::RecordType;
    use std::net::IpAddr;

    #[test]
    fn record_type_follows_address_family() {
        let v4: IpAddr = "203.0.113.10".parse().unwrap();
        let v6: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(RecordType::from(v4), RecordType::A);
        assert_eq!(RecordType::from(v6), RecordType::AAAA);
    }

    /// Cloudflare expects `"type": "AAAA"`, not a tagged object.
    #[test]
    fn record_type_serializes_as_bare_string() {
        assert_eq!(
            serde_json::to_string(&RecordType::AAAA).unwrap(),
            r#""AAAA""#
        );
    }
}
