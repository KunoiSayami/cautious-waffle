mod config {
    use serde_derive::Deserialize;
    use std::fmt::Formatter;

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

    #[derive(Clone, Debug, Deserialize)]
    pub struct Config {
        server: Server,
        clients: Vec<ClientMapper>,
        token: String,
    }

    impl Config {
        pub fn clients(self) -> Vec<ClientMapper> {
            self.clients
        }
        pub fn token(&self) -> &str {
            &self.token
        }

        pub fn get_bind(&self) -> String {
            self.server.to_string()
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Server {
        host: String,
        port: u16,
    }

    impl std::fmt::Display for Server {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

pub use config::Config;
