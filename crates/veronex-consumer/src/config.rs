pub struct Config {
    pub kafka_broker: String,
    pub kafka_group_id: String,
    pub kafka_security_protocol: String,
    pub kafka_sasl_mechanism: String,
    pub kafka_username: String,
    pub kafka_password: String,
    pub clickhouse_url: String,
    pub clickhouse_db: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            kafka_broker: std::env::var("KAFKA_BROKER")
                .unwrap_or_else(|_| "redpanda:9092".to_owned()),
            kafka_group_id: "veronex-consumer".to_owned(),
            kafka_security_protocol: std::env::var("KAFKA_SECURITY_PROTOCOL")
                .unwrap_or_else(|_| "SASL_PLAINTEXT".to_owned()),
            kafka_sasl_mechanism: std::env::var("KAFKA_SASL_MECHANISM")
                .unwrap_or_else(|_| "SCRAM-SHA-512".to_owned()),
            kafka_username: std::env::var("KAFKA_USERNAME")
                .unwrap_or_else(|_| "".to_owned()),
            kafka_password: std::env::var("KAFKA_PASSWORD")
                .unwrap_or_else(|_| "".to_owned()),
            clickhouse_url: std::env::var("CLICKHOUSE_HTTP_URL")
                .unwrap_or_else(|_| "http://clickhouse:8123".to_owned()),
            clickhouse_db: std::env::var("CLICKHOUSE_DB")
                .unwrap_or_else(|_| "veronex".to_owned()),
            clickhouse_user: std::env::var("CLICKHOUSE_USER")
                .unwrap_or_else(|_| "veronex".to_owned()),
            clickhouse_password: std::env::var("CLICKHOUSE_PASSWORD")
                .unwrap_or_else(|_| "veronex".to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sasl_defaults_when_env_unset() {
        // SAFETY: single-threaded test binary; no concurrent env mutation.
        unsafe {
            std::env::remove_var("KAFKA_SECURITY_PROTOCOL");
            std::env::remove_var("KAFKA_SASL_MECHANISM");
            std::env::remove_var("KAFKA_USERNAME");
            std::env::remove_var("KAFKA_PASSWORD");
        }

        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.kafka_security_protocol, "SASL_PLAINTEXT");
        assert_eq!(cfg.kafka_sasl_mechanism, "SCRAM-SHA-512");
        assert_eq!(cfg.kafka_username, "");
        assert_eq!(cfg.kafka_password, "");
        assert_eq!(cfg.kafka_group_id, "veronex-consumer");
    }

    #[test]
    fn sasl_reads_env_vars() {
        // SAFETY: single-threaded test binary; no concurrent env mutation.
        unsafe {
            std::env::set_var("KAFKA_SECURITY_PROTOCOL", "SASL_SSL");
            std::env::set_var("KAFKA_SASL_MECHANISM", "SCRAM-SHA-256");
            std::env::set_var("KAFKA_USERNAME", "testuser");
            std::env::set_var("KAFKA_PASSWORD", "testpass");
        }

        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.kafka_security_protocol, "SASL_SSL");
        assert_eq!(cfg.kafka_sasl_mechanism, "SCRAM-SHA-256");
        assert_eq!(cfg.kafka_username, "testuser");
        assert_eq!(cfg.kafka_password, "testpass");

        unsafe {
            std::env::remove_var("KAFKA_SECURITY_PROTOCOL");
            std::env::remove_var("KAFKA_SASL_MECHANISM");
            std::env::remove_var("KAFKA_USERNAME");
            std::env::remove_var("KAFKA_PASSWORD");
        }
    }

    #[test]
    fn plaintext_protocol_accepted() {
        // SAFETY: single-threaded test binary; no concurrent env mutation.
        unsafe {
            std::env::set_var("KAFKA_SECURITY_PROTOCOL", "PLAINTEXT");
            std::env::remove_var("KAFKA_USERNAME");
            std::env::remove_var("KAFKA_PASSWORD");
        }

        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.kafka_security_protocol, "PLAINTEXT");
        assert_eq!(cfg.kafka_username, "");
        assert_eq!(cfg.kafka_password, "");

        unsafe { std::env::remove_var("KAFKA_SECURITY_PROTOCOL"); }
    }
}
