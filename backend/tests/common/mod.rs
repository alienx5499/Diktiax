//! Shared integration-test helper: a uniquely named graph that is deleted on drop.

use falkordb::{FalkorClientBuilder, FalkorConnectionInfo};

pub struct TestGraph {
    pub url: String,
    pub name: String,
}

impl TestGraph {
    /// Base name comes from `FALKORDB_GRAPH` (else `default_base`) plus a per-run unique suffix.
    pub fn new(default_base: &str) -> Self {
        let url =
            std::env::var("FALKORDB_URL").unwrap_or_else(|_| "falkor://127.0.0.1:6379".into());
        let base = std::env::var("FALKORDB_GRAPH").unwrap_or_else(|_| default_base.into());
        let name = format!("{base}_{}", uuid::Uuid::new_v4().simple());
        Self { url, name }
    }
}

// ponytail: Drop is sync, so cleanup runs on a throwaway thread with its own runtime and is
// best-effort (a hard kill or panic=abort can still leak a graph). Upgrade: a sweep of
// `<base>_*` graphs at suite start.
impl Drop for TestGraph {
    fn drop(&mut self) {
        let (url, name) = (self.url.clone(), self.name.clone());
        let cleanup = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(async {
                let info: FalkorConnectionInfo =
                    url.as_str().try_into().map_err(|e| format!("{e:?}"))?;
                let client = FalkorClientBuilder::new_async()
                    .with_connection_info(info)
                    .build()
                    .await
                    .map_err(|e| e.to_string())?;
                client
                    .select_graph(&name)
                    .delete()
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .join();
        match cleanup {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("warning: failed to delete test graph '{}': {e}", self.name),
            Err(_) => eprintln!("warning: cleanup thread panicked for '{}'", self.name),
        }
    }
}
