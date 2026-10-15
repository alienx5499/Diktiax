//! Optional outbound webhook (`WEBHOOK_URL`). Reports the real delivery result, never a fake one.

use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum WebhookOutcome {
    Skipped(String),
    /// `target` is the redacted URL (scheme and host only): webhook paths often embed the secret.
    Delivered {
        target: String,
        status: u16,
    },
    Failed {
        target: String,
        error: String,
    },
}

#[derive(Clone)]
pub struct Webhook {
    url: Option<String>,
    http: reqwest::Client,
}

impl Webhook {
    pub fn new(url: Option<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| format!("failed to build webhook HTTP client: {e}"))?;
        Ok(Self {
            url: url.filter(|u| !u.trim().is_empty()),
            http,
        })
    }

    pub fn from_env() -> Result<Self, String> {
        Self::new(std::env::var("WEBHOOK_URL").ok())
    }

    pub async fn send(&self, payload: &Value) -> WebhookOutcome {
        let Some(url) = &self.url else {
            return WebhookOutcome::Skipped("WEBHOOK_URL not set".into());
        };
        match self.http.post(url).json(payload).send().await {
            Ok(resp) if resp.status().is_success() => WebhookOutcome::Delivered {
                target: redact(url),
                status: resp.status().as_u16(),
            },
            Ok(resp) => WebhookOutcome::Failed {
                target: redact(url),
                error: format!("HTTP {}", resp.status().as_u16()),
            },
            Err(e) => WebhookOutcome::Failed {
                target: redact(url),
                error: e.without_url().to_string(),
            },
        }
    }
}

/// Keeps only scheme and host, so the token in e.g. `https://hooks.slack.com/services/T0/B0/XYZ`
/// never reaches API responses, logs or exported dossiers.
fn redact(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(u) => format!("{}://{}/…", u.scheme(), u.host_str().unwrap_or("?")),
        Err(_) => "configured webhook".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_path_and_query() {
        assert_eq!(
            redact("https://hooks.slack.com/services/T0/B0/SECRET?x=1"),
            "https://hooks.slack.com/…"
        );
        assert_eq!(redact("not a url"), "configured webhook");
    }

    #[tokio::test]
    async fn unset_url_is_skipped() {
        let hook = Webhook::new(None).expect("client");
        assert_eq!(
            hook.send(&Value::Null).await,
            WebhookOutcome::Skipped("WEBHOOK_URL not set".into())
        );
    }

    #[tokio::test]
    async fn unreachable_url_reports_failure() {
        let hook = Webhook::new(Some("http://127.0.0.1:1/hook".into())).expect("client");
        let outcome = hook.send(&Value::Null).await;
        assert!(
            matches!(&outcome, WebhookOutcome::Failed { target, .. } if target == "http://127.0.0.1/…")
        );
    }
}
