use std::io::Read;
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::model::{AppEntry, INDEX_LIMIT, PROFILE_LIMIT};

pub(crate) const REGISTRY_HOST: &str = "raw.githubusercontent.com";
const REGISTRY_ROOT: &str = "https://raw.githubusercontent.com/dev-five-git/hana-cloud/main/";
pub(crate) const INDEX_URL: &str =
    "https://raw.githubusercontent.com/dev-five-git/hana-cloud/main/registry.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Validators {
    pub(crate) etag: Option<String>,
    pub(crate) last_modified: Option<String>,
}

pub(crate) enum IndexFetch {
    Modified {
        raw: Vec<u8>,
        sha256: String,
        validators: Validators,
    },
    NotModified,
}

pub(crate) struct HttpClient {
    client: Client,
}

impl HttpClient {
    pub(crate) fn new() -> Result<Self, String> {
        let redirect = Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS || !allowed_registry_url(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(redirect)
            .user_agent("HanBeon/0.1 hana-cloud")
            .build()
            .map_err(|error| format!("registry HTTP client를 만들지 못했습니다. ({error})"))?;
        Ok(Self { client })
    }

    pub(crate) fn fetch_index(&self, validators: &Validators) -> Result<IndexFetch, String> {
        let mut request = self.client.get(INDEX_URL);
        if let Some(etag) = validators.etag.as_deref() {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = validators.last_modified.as_deref() {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }

        let response = request
            .send()
            .map_err(|error| format!("registry index를 요청하지 못했습니다. ({error})"))?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(IndexFetch::NotModified);
        }

        let validators = response_validators(&response);
        let raw = response_bytes(response, INDEX_LIMIT, "registry index")?;
        let sha256 = sha256_hex(&raw);
        Ok(IndexFetch::Modified {
            raw,
            sha256,
            validators,
        })
    }

    pub(crate) fn fetch_profile(&self, entry: &AppEntry) -> Result<Vec<u8>, String> {
        let response = self
            .client
            .get(registry_url(&entry.path)?)
            .send()
            .map_err(|error| format!("'{}' profile을 요청하지 못했습니다. ({error})", entry.id))?;
        let raw = response_bytes(response, PROFILE_LIMIT, "app profile")?;
        verify_sha256(&raw, &entry.sha256)?;
        Ok(raw)
    }
}

fn response_validators(response: &Response) -> Validators {
    let text = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    };
    Validators {
        etag: text(ETAG),
        last_modified: text(LAST_MODIFIED),
    }
}

fn response_bytes(response: Response, limit: usize, resource: &str) -> Result<Vec<u8>, String> {
    if !allowed_registry_url(response.url()) {
        return Err(format!("{resource} 응답 host가 허용 범위를 벗어났습니다."));
    }
    if !response.status().is_success() {
        return Err(format!(
            "{resource} 요청이 실패했습니다. (HTTP {})",
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(format!("{resource}가 {limit}바이트 제한을 넘었습니다."));
    }
    read_limited(response, limit)
}

pub(crate) fn registry_url(path: &str) -> Result<reqwest::Url, String> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
    {
        return Err(format!("registry 상대 경로가 안전하지 않습니다. ({path})"));
    }

    let base = reqwest::Url::parse(REGISTRY_ROOT)
        .map_err(|error| format!("registry base URL이 잘못됐습니다. ({error})"))?;
    let url = base
        .join(path)
        .map_err(|error| format!("registry URL을 만들지 못했습니다. ({error})"))?;
    allowed_registry_url(&url)
        .then_some(url)
        .ok_or_else(|| "registry URL host가 허용 범위를 벗어났습니다.".to_string())
}

pub(crate) fn allowed_registry_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(REGISTRY_HOST)
}

pub(crate) fn read_limited(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("registry 응답을 읽지 못했습니다. ({error})"))?;
    if bytes.len() > limit {
        return Err(format!("registry 응답이 {limit}바이트 제한을 넘었습니다."));
    }
    Ok(bytes)
}

pub(crate) fn sha256_hex(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn verify_sha256(raw: &[u8], expected: &str) -> Result<(), String> {
    let actual = sha256_hex(raw);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "registry SHA-256이 다릅니다. (예상 {expected}, 실제 {actual})"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn registry_urls_are_fixed_to_https_and_the_hana_repository() {
        let index = registry_url("registry.json").unwrap();
        let profile = registry_url("apps/pdf-viewer.json").unwrap();

        assert_eq!(index.as_str(), INDEX_URL);
        assert_eq!(profile.scheme(), "https");
        assert_eq!(profile.host_str(), Some(REGISTRY_HOST));
        assert_eq!(
            profile.path(),
            "/dev-five-git/hana-cloud/main/apps/pdf-viewer.json"
        );
    }

    #[test]
    fn registry_urls_reject_traversal_and_absolute_input() {
        assert!(registry_url("../secrets.json").is_err());
        assert!(registry_url("https://example.com/profile.json").is_err());
        assert!(registry_url("apps\\profile.json").is_err());
    }

    #[test]
    fn redirect_target_must_keep_https_and_the_registry_host() {
        assert!(allowed_registry_url(
            &reqwest::Url::parse(INDEX_URL).unwrap()
        ));
        assert!(!allowed_registry_url(
            &reqwest::Url::parse(
                "http://raw.githubusercontent.com/dev-five-git/hana-cloud/main/registry.json"
            )
            .unwrap()
        ));
        assert!(!allowed_registry_url(
            &reqwest::Url::parse("https://example.com/registry.json").unwrap()
        ));
    }

    #[test]
    fn limited_reader_accepts_the_limit_and_rejects_one_byte_more() {
        assert_eq!(
            read_limited(Cursor::new(vec![7; 4]), 4).unwrap(),
            vec![7; 4]
        );
        assert!(read_limited(Cursor::new(vec![7; 5]), 4).is_err());
    }

    #[test]
    fn sha256_must_match_before_a_profile_is_accepted() {
        let raw = b"hana";
        let expected = sha256_hex(raw);

        assert!(verify_sha256(raw, &expected).is_ok());
        assert!(verify_sha256(raw, &"0".repeat(64)).is_err());
    }
}
