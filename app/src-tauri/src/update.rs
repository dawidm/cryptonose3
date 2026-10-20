//! Checks GitHub releases for a newer version.

use serde::Serialize;
use serde_json::Value;

const RELEASES: &str = "https://api.github.com/repos/dawidm/cryptonose3/releases/latest";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub url: String,
}

fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

pub fn newer(current: &str, release: &Value) -> Option<Release> {
    let tag = release["tag_name"].as_str()?;
    if release["draft"] == true || release["prerelease"] == true {
        return None;
    }
    (parse_version(tag)? > parse_version(current)?).then(|| Release {
        version: tag.trim_start_matches('v').to_string(),
        url: release["html_url"].as_str().unwrap_or("https://github.com/dawidm/cryptonose3/releases").to_string(),
    })
}

pub async fn check(current: &str) -> anyhow::Result<Option<Release>> {
    let resp = cn_exchanges::util::http().get(RELEASES).header("Accept", "application/vnd.github+json").send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    let v: Value = resp.error_for_status()?.json().await?;
    Ok(newer(current, &v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compares_versions() {
        let r = json!({"tag_name": "v3.1.0", "html_url": "https://x"});
        assert_eq!(newer("3.0.9", &r).unwrap().version, "3.1.0");
        assert!(newer("3.1.0", &r).is_none());
        assert!(newer("3.0.0", &json!({"tag_name": "v3.2.0", "prerelease": true})).is_none());
    }
}
