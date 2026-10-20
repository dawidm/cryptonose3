use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("cryptonose/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client")
    })
}

async fn check(resp: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("HTTP {status}: {}", body.chars().take(300).collect::<String>());
    }
    Ok(resp)
}

pub async fn get_json<T: DeserializeOwned>(url: &str) -> anyhow::Result<T> {
    let resp = check(http().get(url).send().await.with_context(|| format!("GET {url}"))?).await?;
    resp.json().await.with_context(|| format!("decoding {url}"))
}

pub async fn post_json<T: DeserializeOwned>(url: &str, body: &Value) -> anyhow::Result<T> {
    let resp = check(http().post(url).json(body).send().await.with_context(|| format!("POST {url}"))?).await?;
    resp.json().await.with_context(|| format!("decoding {url}"))
}

/// Number from a JSON number or numeric string.
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// RFC 3339 timestamp to unix ms.
pub fn rfc3339_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}
