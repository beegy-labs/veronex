pub struct Config {
    pub kafka_broker: String,
    pub kafka_group_id: String,
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
