//! Device HUB 本体の更新確認 (読み取り専用 — ダウンロードや自己更新はしない)。
//! 参考: narurea/device-hub の GitHub Releases 確認。

use crate::UpdateError;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(6);
const UA: &str = "DeviceHub-UpdateCheck";

#[derive(Clone, Debug, PartialEq)]
pub struct AppUpdate {
    pub latest: String,
    pub newer: bool,
    pub url: String,
    pub notes: Option<String>,
    pub from_release: bool,
}

/// `repo` = `owner/name`。Release が無い場合は既定ブランチの Cargo.toml を見る。
pub fn check_app(repo: &str, branch: &str, current: &str) -> Result<AppUpdate, UpdateError> {
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let api = format!("https://api.github.com/repos/{repo}/releases/latest");
    match agent
        .get(&api)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => {
            let v: serde_json::Value = r
                .into_json()
                .map_err(|e| UpdateError(format!("release の解析に失敗: {e}")))?;
            from_release_json(&v, repo, current)
        }
        Err(ureq::Error::Status(404, _)) => {
            let raw = format!(
                "https://raw.githubusercontent.com/{repo}/{branch}/crates/device-hub/Cargo.toml"
            );
            let text = agent
                .get(&raw)
                .set("User-Agent", UA)
                .call()
                .map_err(|e| UpdateError(format!("バージョン情報を取得できません: {e}")))?
                .into_string()
                .map_err(|e| UpdateError(e.to_string()))?;
            let latest = package_version(&text)
                .ok_or_else(|| UpdateError("Cargo.toml に version がありません".into()))?;
            Ok(AppUpdate {
                newer: crate::version_newer(&latest, current),
                latest,
                url: format!("https://github.com/{repo}/commits/{branch}"),
                notes: None,
                from_release: false,
            })
        }
        Err(ureq::Error::Status(code, _)) => {
            Err(UpdateError(format!("GitHub が {code} を返しました")))
        }
        Err(e) => Err(UpdateError(format!("接続できません: {e}"))),
    }
}

fn from_release_json(
    v: &serde_json::Value,
    repo: &str,
    current: &str,
) -> Result<AppUpdate, UpdateError> {
    let tag = v["tag_name"]
        .as_str()
        .ok_or_else(|| UpdateError("release に tag_name がありません".into()))?;
    let latest = tag.trim_start_matches(['v', 'V']).to_string();
    let url = v["html_url"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://github.com/{repo}/releases"));
    let notes = v["body"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut n: String = s.chars().take(600).collect();
            if s.chars().count() > 600 {
                n.push('…');
            }
            n
        });
    Ok(AppUpdate {
        newer: crate::version_newer(&latest, current),
        latest,
        url,
        notes,
        from_release: true,
    })
}

fn package_version(toml: &str) -> Option<String> {
    let mut in_pkg = false;
    for line in toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_pkg = l == "[package]";
            continue;
        }
        if in_pkg {
            if let Some(rest) = l.strip_prefix("version") {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    let v = v.trim().trim_matches('"');
                    if !v.is_empty() && v.contains('.') {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_package_version_only() {
        let t = concat!(
            "[package]
",
            "name = \"device-hub\"
",
            "version = \"0.2.7\"

",
            "[dependencies]
",
            "version = \"9\"
",
        );
        assert_eq!(package_version(t).as_deref(), Some("0.2.7"));
        assert_eq!(package_version("[dependencies]
version = \"1.0\"
"), None);
    }

    #[test]
    fn release_json() {
        let v = serde_json::json!({
            "tag_name": "v0.3.0",
            "html_url": "https://github.com/o/r/releases/tag/v0.3.0",
            "body": "  update notes  "
        });
        let u = from_release_json(&v, "o/r", "0.2.6").unwrap();
        assert_eq!(u.latest, "0.3.0");
        assert!(u.newer && u.from_release);
        assert_eq!(u.notes.as_deref(), Some("update notes"));
        assert!(!from_release_json(&v, "o/r", "0.3.0").unwrap().newer);
    }
}
