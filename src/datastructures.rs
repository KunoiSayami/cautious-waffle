mod config {
    use anyhow::anyhow;
    use serde_derive::Deserialize;
    use std::fmt::Formatter;

    #[derive(Clone, Debug, Deserialize)]
    pub struct ZoneMapper {
        domain: String,
        zone: String,
    }

    impl ZoneMapper {
        pub fn domain(&self) -> &str {
            &self.domain
        }
        pub fn zone(&self) -> &str {
            &self.zone
        }
        pub fn new(domain: String, zone: String) -> Self {
            Self { domain, zone }
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct ClientMapper {
        uuid: String,
        target: Vec<String>,
    }

    impl ClientMapper {
        pub fn uuid(&self) -> &String {
            &self.uuid
        }
        pub fn target(&self) -> &Vec<String> {
            &self.target
        }
    }

    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct ClientMapperSingle {
        uuid: String,
        target: Option<String>,
    }

    impl ClientMapperSingle {
        pub fn uuid(&self) -> &str {
            &self.uuid
        }

        pub fn target(&self) -> &str {
            match self.target {
                None => self.uuid(),
                Some(ref s) => s,
            }
        }
    }

    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct Relay {
        enabled: bool,
        target: Vec<String>,
        clients: Vec<ClientMapperSingle>,
        proxy: Option<String>,
    }

    impl Relay {
        pub fn enabled(&self) -> bool {
            self.enabled
        }
        pub fn target(&self) -> Vec<String> {
            self.target.clone()
        }

        pub fn clients(&self) -> &Vec<ClientMapperSingle> {
            &self.clients
        }
        pub fn proxy(&self) -> &Option<String> {
            &self.proxy
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Config {
        server: Server,
        #[serde(default)]
        client: Vec<ClientMapper>,
        // Can be empty if relay
        #[serde(default)]
        zones: Vec<ZoneMapper>,
        #[serde(default)]
        relay: Relay,
        // Can be option if relay
        #[serde(default)]
        token: String,
        column_ip: Option<String>,
    }

    impl Config {
        pub fn clients(&self) -> &Vec<ClientMapper> {
            &self.client
        }

        pub fn token(&self) -> &str {
            &self.token
        }

        pub fn get_bind(&self) -> String {
            self.server.to_string()
        }

        pub fn zones(&self) -> &Vec<ZoneMapper> {
            &self.zones
        }

        pub fn is_relay_mode(&self) -> bool {
            return self.relay.enabled();
        }

        pub fn relay(self) -> Relay {
            self.relay
        }
        pub fn column_ip(&self) -> &Option<String> {
            &self.column_ip
        }

        pub async fn try_from_file(location: &str) -> anyhow::Result<Self> {
            let config: Self = toml::from_str(
                &tokio::fs::read_to_string(&location)
                    .await
                    .map_err(|e| anyhow!("Unable read {:?}: {:?}", &location, e))?,
            )
            .map_err(|e| anyhow!("Unable serialize configure toml: {:?}", e))?;

            if !config.check_config() {
                return Err(anyhow!(
                    "Config check failed. if not use relay mode, please specify token and zone"
                ));
            }

            Ok(config)
        }

        #[must_use]
        fn check_config(&self) -> bool {
            self.is_relay_mode()
                || (!self.token.is_empty() && !self.zones.is_empty() && !self.client.is_empty())
        }

        pub fn enable_query(&self) -> bool {
            self.server.enable_query()
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Server {
        host: String,
        port: u16,
        #[serde(default)]
        enable_query: bool,
    }

    impl Server {
        pub fn enable_query(&self) -> bool {
            self.enable_query
        }
    }

    impl std::fmt::Display for Server {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

mod web {
    use serde_derive::{Deserialize, Serialize};

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct PostData {
        ip: String,
    }

    impl PostData {
        pub fn ip(&self) -> &str {
            &self.ip
        }
        pub fn new(ip: String) -> Self {
            Self { ip }
        }
    }
}

mod relay {
    use super::RelayConfig;
    use anyhow::anyhow;
    use log::warn;
    use serde_derive::Deserialize;
    use std::collections::HashMap;

    const DISABLE_URL_WARNING: &str = "DISABLE_URL_WARNING";

    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct Relay {
        enabled: bool,
        target: Vec<String>,
        clients: HashMap<String, String>,
    }

    impl Relay {
        pub fn enabled(&self) -> bool {
            self.enabled
        }

        pub fn target(&self) -> &Vec<String> {
            &self.target
        }

        pub fn clients(&self) -> &HashMap<String, String> {
            &self.clients
        }
    }

    impl TryFrom<RelayConfig> for Relay {
        type Error = anyhow::Error;

        fn try_from(value: RelayConfig) -> Result<Self, Self::Error> {
            if !value.enabled() {
                return Ok(Default::default());
            }
            let targets = value.target();

            // Check clients is empty
            if value.clients().is_empty() {
                return Err(anyhow!("Clients is empty."));
            }

            // Check if disable warning
            let disable_warning = std::env::var(DISABLE_URL_WARNING)
                .map(|s| s.parse::<i64>().unwrap_or_default() != 0)
                .unwrap_or_default();

            // Variable to store is warning has sent already
            let mut warning_sent = false;

            if !disable_warning {
                for target in targets {
                    if !['=', '/', '?'].iter().any(|x| target.ends_with(*x)) {
                        warn!("{:?} is not ends with `=`, `/` or `?`", target);
                        warning_sent = true;
                    }
                }
                if warning_sent {
                    warn!(
                        "You can disable this warning by set `{}` environment variable to `1`",
                        DISABLE_URL_WARNING
                    );
                }
            }

            let mut m = HashMap::new();
            // Insert client map
            for client in value.clients() {
                m.insert(client.uuid().to_string(), client.target().to_string());
            }

            Ok(Self {
                enabled: true,
                target: value.target(),
                clients: m,
            })
        }
    }
}

pub use config::ZoneMapper;
pub use config::{Config, Relay as RelayConfig};
pub use relay::Relay;
pub use web::PostData;
