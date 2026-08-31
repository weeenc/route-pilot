use reqwest::{header::LOCATION, redirect::Policy, Client, Url};
use serde::Serialize;
use std::time::Duration;

const LATEST_RELEASE_URL: &str = "https://github.com/weeenc/route-pilot/releases/latest";
const RELEASE_PATH_PREFIX: &str = "/weeenc/route-pilot/releases/tag/";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    version: String,
    name: String,
    url: String,
}

fn release_from_location(location: &str) -> Result<ReleaseInfo, String> {
    let url = Url::parse(location).map_err(|_| "GitHub returned an invalid release URL")?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return Err("GitHub returned an invalid release URL".to_owned());
    }

    let tag = url
        .path()
        .strip_prefix(RELEASE_PATH_PREFIX)
        .filter(|tag| !tag.is_empty() && !tag.contains('/'))
        .ok_or_else(|| "GitHub returned an invalid release tag".to_owned())?;
    let version = tag
        .strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag)
        .to_owned();

    Ok(ReleaseInfo {
        name: format!("RoutePilot v{version}"),
        version,
        url: url.to_string(),
    })
}

#[tauri::command]
pub async fn get_latest_release() -> Result<ReleaseInfo, String> {
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| "Unable to prepare the update check")?;
    let response = client
        .head(LATEST_RELEASE_URL)
        .header("User-Agent", "RoutePilot update checker")
        .send()
        .await
        .map_err(|_| "Unable to reach GitHub Releases")?;

    if !response.status().is_redirection() {
        return Err("GitHub did not return a latest release".to_owned());
    }

    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "GitHub did not return a release URL".to_owned())?;
    release_from_location(location)
}

#[cfg(test)]
mod tests {
    use super::release_from_location;

    #[test]
    fn parses_routepilot_release_redirects() {
        let release =
            release_from_location("https://github.com/weeenc/route-pilot/releases/tag/v0.2.0")
                .expect("valid release redirect");
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.name, "RoutePilot v0.2.0");
    }

    #[test]
    fn rejects_redirects_outside_the_repository() {
        assert!(release_from_location(
            "https://github.com/someone/another-project/releases/tag/v9.0.0"
        )
        .is_err());
        assert!(release_from_location("https://example.com/v9.0.0").is_err());
    }
}
