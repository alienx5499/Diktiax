use std::env;

#[derive(Clone)]
pub struct AppConfig {
    pub falkordb_url: String,
    pub graph_name: String,
    pub server_port: u16,
    pub server_host: String,
    /// Password of the read-only evidence user: `FALKORDB_READER_PASSWORD`, else random per process.
    pub reader_password: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let falkordb_url =
            env::var("FALKORDB_URL").unwrap_or_else(|_| "falkor://127.0.0.1:6379".to_string());
        let graph_name = env::var("FALKORDB_GRAPH").unwrap_or_else(|_| "diktiax".to_string());
        let server_port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3001);
        let server_host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        let reader_password = env::var("FALKORDB_READER_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                )
            });

        Self {
            falkordb_url,
            graph_name,
            server_port,
            server_host,
            reader_password,
        }
    }
}
