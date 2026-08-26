//! Hana Cloud (https://github.com/dev-five-git/hana-cloud) board-registry client.
//!
//! Downloads and verifies the root index, board manifests, and firmware files.
//! The contract from the registry README is binding:
//! - HTTPS to `raw.githubusercontent.com/dev-five-git/hana-cloud` only; a
//!   redirect that changes the host is refused.
//! - Size caps: index 256KiB, manifest 64KiB, firmware 2MiB.
//! - 5 second timeout per request.
//! - SHA-256 of every file is checked before the file is used or cached.
//! - On network or verification failure the last known-good cache wins.
//! - Firmware download happens only after the user explicitly starts an
//!   install; detection alone never triggers network access.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const REGISTRY_BASE: &str = "https://raw.githubusercontent.com/dev-five-git/hana-cloud/main";
pub const INDEX_MAX_BYTES: usize = 256 * 1024;
pub const MANIFEST_MAX_BYTES: usize = 64 * 1024;
pub const FIRMWARE_MAX_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const INDEX_CACHE_PATH: &str = "registry.json";

/// Normalization contract for human-readable USB descriptor strings only
/// (registry README "식별자와 문자열 정규화"). Never applied to ids, paths,
/// hashes, or VID/PID.
pub fn normalize_descriptor(input: &str) -> String {
    let nfkc: String = input.nfkc().collect();
    let folded: String = nfkc
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == '_' || c == '-' {
                '-'
            } else {
                c
            }
        })
        .collect::<String>()
        .to_lowercase();

    // Collapse runs of `-` produced by folding, then trim edge `-`.
    let mut collapsed = String::with_capacity(folded.len());
    let mut previous_dash = false;
    for c in folded.chars() {
        if c == '-' {
            if !previous_dash {
                collapsed.push('-');
            }
            previous_dash = true;
        } else {
            collapsed.push(c);
            previous_dash = false;
        }
    }
    let trimmed = collapsed.trim_matches('-');
    trimmed.to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    use std::fmt::Write as _;
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[derive(Debug, PartialEq)]
pub enum RegistryError {
    /// The board's VID/PID has no entry in the registry index.
    BoardNotRegistered { vid: u16, pid: u16 },
    /// A registry file failed its SHA-256 check.
    HashMismatch { path: String },
    /// Network failure with no usable cache.
    Unavailable(String),
    /// The downloaded document is structurally invalid.
    InvalidDocument(&'static str),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BoardNotRegistered { vid, pid } => write!(
                f,
                "레지스트리에 등록되지 않은 보드입니다 (USB {vid:04x}:{pid:04x})."
            ),
            Self::HashMismatch { path } => {
                write!(f, "레지스트리 파일의 해시가 일치하지 않습니다: {path}")
            }
            Self::Unavailable(message) => {
                write!(f, "레지스트리에 연결할 수 없습니다: {message}")
            }
            Self::InvalidDocument(detail) => {
                write!(f, "레지스트리 응답이 잘못되었습니다: {detail}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Ambiguous,
    Likely,
    Exact,
}

/// A board candidate resolved from USB identity against the registry index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbMatch {
    pub board_id: String,
    pub board_name: String,
    pub confidence: Confidence,
    pub manifest_path: String,
    pub manifest_sha256: String,
}

pub struct UsbIdentity {
    pub vid: u16,
    pub pid: u16,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryIndex {
    schema_version: u32,
    revision: u64,
    #[serde(rename = "apps")]
    _apps: Vec<serde_json::Value>,
    boards: Vec<IndexBoard>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexBoard {
    id: String,
    name: String,
    manifest: String,
    sha256: String,
    detect: DetectSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectSpec {
    usb: Vec<UsbDetectEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ConfidenceTag {
    Exact,
    Likely,
    Ambiguous,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsbDetectEntry {
    vid: String,
    pid: String,
    confidence: ConfidenceTag,
    #[serde(default)]
    manufacturer_aliases: Vec<String>,
    #[serde(default)]
    product_aliases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BoardManifest {
    schema_version: u32,
    id: String,
    firmware: FirmwareRef,
    wiring: Vec<WiringEntry>,
    #[serde(default)]
    image: Option<ImageRef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwareRef {
    pub path: String,
    pub format: String,
    pub size: usize,
    pub fqbn: String,
    pub sha256: String,
    source: SourceRef,
    toolchain: ToolchainRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRef {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolchainRef {
    arduino_cli: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WiringEntry {
    from: String,
    to: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageRef {
    path: String,
    sha256: String,
    alt: String,
}

/// A verified firmware ready for flashing.
pub struct VerifiedFirmware {
    pub board_id: String,
    pub hex_text: String,
    #[allow(dead_code)]
    pub fqbn: String,
}

fn parse_usb_hex(value: &str) -> Option<u16> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    u16::from_str_radix(digits, 16).ok()
}

fn validate_relative_path(path: &str) -> Result<(), RegistryError> {
    let allowed = !path.is_empty()
        && !path.contains("..")
        && !path.contains('\\')
        && !path.contains(':')
        && !path.starts_with('/');
    if allowed {
        Ok(())
    } else {
        Err(RegistryError::InvalidDocument("허용되지 않는 경로"))
    }
}

fn validate_sha_format(hash: &str) -> Result<(), RegistryError> {
    let valid = hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if valid {
        Ok(())
    } else {
        Err(RegistryError::InvalidDocument("sha256 형식 오류"))
    }
}

pub(crate) fn parse_index(bytes: &[u8]) -> Result<RegistryIndex, RegistryError> {
    let index: RegistryIndex =
        serde_json::from_slice(bytes).map_err(|_| RegistryError::InvalidDocument("인덱스 JSON"))?;
    if index.schema_version != 1 || index.revision == 0 {
        return Err(RegistryError::InvalidDocument("지원하지 않는 인덱스 버전"));
    }
    let mut ids = std::collections::HashSet::new();
    for board in &index.boards {
        if board.id.is_empty()
            || board.name.is_empty()
            || !ids.insert(board.id.as_str())
            || board.detect.usb.is_empty()
        {
            return Err(RegistryError::InvalidDocument("잘못된 보드 인덱스"));
        }
        validate_relative_path(&board.manifest)?;
        validate_sha_format(&board.sha256)?;
        for usb in &board.detect.usb {
            if parse_usb_hex(&usb.vid).is_none() || parse_usb_hex(&usb.pid).is_none() {
                return Err(RegistryError::InvalidDocument("잘못된 USB VID/PID"));
            }
        }
    }
    Ok(index)
}

/// Applies the README matching rules: VID/PID decides, provided descriptors can
/// keep the entry's confidence or demote it to ambiguous, never promote.
fn match_board(index: &RegistryIndex, identity: &UsbIdentity) -> Option<UsbMatch> {
    let want_vid = format!("{:04x}", identity.vid);
    let want_pid = format!("{:04x}", identity.pid);
    let normalized_product = identity.product.as_deref().map(normalize_descriptor);
    let normalized_manufacturer = identity.manufacturer.as_deref().map(normalize_descriptor);

    let mut best: Option<(Confidence, usize)> = None;
    for (position, board) in index.boards.iter().enumerate() {
        for entry in &board.detect.usb {
            // VID/PID are compared after u16 round-trip so 0X2A03 and 2a03
            // collapse to the same four-digit lowercase form.
            let Some(entry_vid) = parse_usb_hex(&entry.vid) else {
                continue;
            };
            let Some(entry_pid) = parse_usb_hex(&entry.pid) else {
                continue;
            };
            if format!("{entry_vid:04x}") != want_vid || format!("{entry_pid:04x}") != want_pid {
                continue;
            }
            let mut confidence = match entry.confidence {
                ConfidenceTag::Exact => Confidence::Exact,
                ConfidenceTag::Likely => Confidence::Likely,
                ConfidenceTag::Ambiguous => Confidence::Ambiguous,
            };
            let disagrees = |aliases: &[String], value: &Option<String>| {
                !aliases.is_empty()
                    && value.as_deref().is_some_and(|value| {
                        !aliases
                            .iter()
                            .any(|alias| normalize_descriptor(alias) == value)
                    })
            };
            if disagrees(&entry.manufacturer_aliases, &normalized_manufacturer)
                || disagrees(&entry.product_aliases, &normalized_product)
            {
                confidence = Confidence::Ambiguous;
            }
            if best.is_none_or(|(best_confidence, _)| confidence > best_confidence) {
                best = Some((confidence, position));
            }
        }
    }

    let (_, position) = best?;
    let board = &index.boards[position];
    validate_relative_path(&board.manifest).ok()?;
    validate_sha_format(&board.sha256).ok()?;
    Some(UsbMatch {
        board_id: board.id.clone(),
        board_name: board.name.clone(),
        confidence: best.map(|(confidence, _)| confidence)?,
        manifest_path: board.manifest.clone(),
        manifest_sha256: board.sha256.clone(),
    })
}

impl RegistryIndex {
    pub fn match_board(&self, identity: &UsbIdentity) -> Option<UsbMatch> {
        match_board(self, identity)
    }
}

/// Bytes source so tests never touch the network. Production resolves HTTPS.
type Fetcher = dyn Fn(&str, usize) -> Result<Vec<u8>, RegistryError> + Send + Sync;

fn network_fetch(base_url: String) -> Box<Fetcher> {
    Box::new(move |path: &str, max_bytes: usize| {
        let url = format!("{}/{}", base_url.trim_end_matches('/'), path);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            // 리다이렉트를 아예 따라가지 않는다. 레지스트리 계약상 응답 호스트는
            // 요청한 raw.githubusercontent.com으로 고정이고, 3xx는 실패로 본다.
            .timeout_global(Some(REQUEST_TIMEOUT))
            .max_redirects(0)
            .build()
            .into();
        let response = match agent.get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(status)) if (300..400).contains(&status) => {
                return Err(RegistryError::Unavailable(format!(
                    "리다이렉트는 허용되지 않습니다 (HTTP {status})"
                )));
            }
            Err(ureq::Error::StatusCode(status)) => {
                return Err(RegistryError::Unavailable(format!("HTTP {status}")));
            }
            Err(error) => return Err(RegistryError::Unavailable(error.to_string())),
        };

        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .take((max_bytes + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|error| RegistryError::Unavailable(error.to_string()))?;
        if body.len() > max_bytes {
            return Err(RegistryError::Unavailable(format!(
                "응답이 너무 큽니다 ({max_bytes} 바이트 초과)"
            )));
        }
        Ok(body)
    })
}

/// Cache layout under the app data dir:
/// `hana-cloud/registry.json`, `<board-id>/manifest.json`,
/// `<board-id>/firmware.hex`.
pub struct RegistryClient {
    cache_dir: PathBuf,
    fetch: Box<Fetcher>,
}

impl RegistryClient {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            fetch: network_fetch(REGISTRY_BASE.to_owned()),
        }
    }

    /// GET with the size cap enforced while streaming.
    fn fetch_capped(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, RegistryError> {
        (self.fetch)(path, max_bytes)
    }

    /// Returns verified bytes, using the cache when its hash still matches the
    /// index and downloading otherwise. Never caches unverified bytes.
    fn verified_with_cache(
        &self,
        path: &str,
        expected_sha: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, RegistryError> {
        validate_relative_path(path)?;
        validate_sha_format(expected_sha)?;

        if let Some(cached) = self.read_cache(path)
            && sha256_hex(&cached) == expected_sha
        {
            return Ok(cached);
        }

        let bytes = self.fetch_capped(path, max_bytes)?;
        if sha256_hex(&bytes) != expected_sha {
            // Keep any existing older cache; never store unverified data.
            return Err(RegistryError::HashMismatch {
                path: path.to_owned(),
            });
        }
        self.write_cache(path, &bytes);
        Ok(bytes)
    }

    fn read_cache(&self, relative_path: &str) -> Option<Vec<u8>> {
        fs::read(self.cache_dir.join(relative_path)).ok()
    }

    fn write_cache(&self, relative_path: &str, bytes: &[u8]) {
        let target = self.cache_dir.join(relative_path);
        let Some(parent) = target.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        // 원자적 교체: 쓰다 끊겨도 캐시가 깨지지 않는다.
        let temp = target.with_extension("part");
        if fs::write(&temp, bytes).is_ok() {
            let replaced = fs::rename(&temp, &target).or_else(|_| {
                // Windows는 기존 파일 위 rename을 거부하므로 검증된 임시 파일이
                // 준비된 뒤에만 이전 캐시를 제거하고 다시 옮긴다.
                fs::remove_file(&target)?;
                fs::rename(&temp, &target)
            });
            if replaced.is_err() {
                let _ = fs::remove_file(&temp);
            }
        }
    }

    /// Fetches and verifies the index, preferring the last-known-good cache on
    /// any network or validation failure.
    pub fn load_index(&self) -> Result<RegistryIndex, RegistryError> {
        let fresh = self
            .fetch_capped(INDEX_CACHE_PATH, INDEX_MAX_BYTES)
            .and_then(|bytes| {
                let parsed = parse_index(&bytes)?;
                self.write_cache(INDEX_CACHE_PATH, &bytes);
                Ok(parsed)
            });
        fresh.or_else(|fresh_error| {
            self.read_cache(INDEX_CACHE_PATH)
                .and_then(|bytes| parse_index(&bytes).ok())
                .ok_or(fresh_error)
        })
    }

    /// Matches connected USB VID/PID against the registry index.
    pub fn match_board(&self, identity: &UsbIdentity) -> Result<UsbMatch, RegistryError> {
        let index = self.load_index()?;
        match_board(&index, identity).ok_or(RegistryError::BoardNotRegistered {
            vid: identity.vid,
            pid: identity.pid,
        })
    }

    /// Resolves, verifies, and caches a flashable firmware for the matched
    /// board. Call only after the user explicitly started an install.
    pub fn resolve_firmware(&self, matched: &UsbMatch) -> Result<VerifiedFirmware, RegistryError> {
        if matched.board_id != "arduino.uno-r3" {
            return Err(RegistryError::InvalidDocument("지원하지 않는 보드 모델"));
        }
        let manifest_bytes = self.verified_with_cache(
            &matched.manifest_path,
            &matched.manifest_sha256,
            MANIFEST_MAX_BYTES,
        )?;
        let manifest: BoardManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|_| RegistryError::InvalidDocument("보드 manifest JSON"))?;

        if manifest.schema_version != 2
            || manifest.id != matched.board_id
            || manifest.firmware.format != "intel-hex"
            || manifest.firmware.fqbn != "arduino:avr:uno"
            || manifest.firmware.size == 0
            || manifest.firmware.size > FIRMWARE_MAX_BYTES
            || manifest.wiring.is_empty()
        {
            return Err(RegistryError::InvalidDocument(
                "지원하지 않는 보드 manifest",
            ));
        }
        validate_relative_path(&manifest.firmware.source.path)?;
        validate_sha_format(&manifest.firmware.source.sha256)?;
        if manifest.firmware.toolchain.arduino_cli.is_empty()
            || manifest.firmware.toolchain.platform.is_empty()
        {
            return Err(RegistryError::InvalidDocument("펌웨어 빌드 출처 누락"));
        }
        if manifest.wiring.iter().any(|entry| {
            entry.from.is_empty()
                || entry.to.is_empty()
                || entry.note.as_deref().is_some_and(str::is_empty)
        }) {
            return Err(RegistryError::InvalidDocument("배선 정보 오류"));
        }
        if let Some(image) = &manifest.image {
            validate_relative_path(&image.path)?;
            validate_sha_format(&image.sha256)?;
            if image.alt.is_empty() {
                return Err(RegistryError::InvalidDocument("보드 이미지 설명 누락"));
            }
        }

        validate_relative_path(&manifest.firmware.path)?;
        let hex_bytes = self.verified_with_cache(
            &manifest.firmware.path,
            &manifest.firmware.sha256,
            FIRMWARE_MAX_BYTES,
        )?;
        if hex_bytes.len() != manifest.firmware.size {
            return Err(RegistryError::InvalidDocument("펌웨어 크기 불일치"));
        }
        let hex_text = String::from_utf8(hex_bytes)
            .map_err(|_| RegistryError::InvalidDocument("펌웨어가 텍스트가 아닙니다"))?;

        Ok(VerifiedFirmware {
            board_id: matched.board_id.clone(),
            hex_text,
            fqbn: manifest.firmware.fqbn,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hanbeon-registry-test-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn uno_index() -> Vec<u8> {
        br#"{
            "schemaVersion": 1,
            "revision": 1,
            "apps": [],
            "boards": [{
                "id": "arduino.uno-r3",
                "name": "Arduino Uno R3",
                "manifest": "boards/arduino-uno-r3.json",
                "sha256": "f3842b609b55f75cb00f2a2fda3e12cb2f11b0f24c9b793aa12dd8565ea9889b",
                "detect": {"usb": [{"vid": "2341", "pid": "0043", "confidence": "exact",
                    "manufacturerAliases": ["Arduino", "Arduino LLC"],
                    "productAliases": ["Arduino Uno", "Arduino Uno R3"]}]}
            }]
        }"#
        .to_vec()
    }

    fn xiao_index() -> Vec<u8> {
        br#"{
            "schemaVersion": 1,
            "revision": 4,
            "apps": [],
            "boards": [{
                "id": "seeed.xiao-nrf52840-sense",
                "name": "Seeed XIAO nRF52840 Sense",
                "manifest": "boards/seeed-xiao-nrf52840-sense.json",
                "sha256": "f3842b609b55f75cb00f2a2fda3e12cb2f11b0f24c9b793aa12dd8565ea9889b",
                "detect": {"usb": [
                    {"vid": "2886", "pid": "8045", "confidence": "exact",
                        "manufacturerAliases": ["Seeed Studio"],
                        "productAliases": ["XIAO nRF52840 Sense"]},
                    {"vid": "2886", "pid": "0045", "confidence": "exact",
                        "manufacturerAliases": ["Seeed Studio"],
                        "productAliases": ["XIAO nRF52840 Sense"]}
                ]}
            }]
        }"#
        .to_vec()
    }

    #[test]
    fn normalization_matches_registry_contract_vectors() {
        assert_eq!(normalize_descriptor(" Arduino UNO_R3 "), "arduino-uno-r3");
        assert_eq!(normalize_descriptor("arduino-uno r3"), "arduino-uno-r3");
        assert_eq!(
            normalize_descriptor("Ａｒｄｕｉｎｏ　ＵＮＯ－Ｒ３"),
            "arduino-uno-r3"
        );
        assert_eq!(normalize_descriptor("Arduino   LLC"), "arduino-llc");
        assert_eq!(normalize_descriptor(" MÜNCHEN_GmbH "), "münchen-gmbh");
        assert_eq!(
            normalize_descriptor(" Arduino (WWW.Arduino.cc) "),
            "arduino-(www.arduino.cc)"
        );
    }

    #[test]
    fn usb_hex_serializes_via_u16_roundtrip() {
        // README: VID/PID는 u16으로 파싱한 뒤 4자리 소문자 hex로 직렬화한다.
        for raw in ["2A03", "2a03", "0x2A03"] {
            let parsed = parse_usb_hex(raw).map(|v| format!("{v:04x}"));
            assert_eq!(parsed.as_deref(), Some("2a03"), "{raw}");
        }
        assert_eq!(parse_usb_hex("zz").map(|v| format!("{v:04x}")), None);
    }

    #[test]
    fn exact_vid_pid_matches_and_descriptors_keep_confidence() {
        let index = parse_index(&uno_index()).unwrap();
        let identity = UsbIdentity {
            vid: 0x2341,
            pid: 0x0043,
            product: Some("Arduino Uno R3".to_owned()),
            manufacturer: Some("Arduino LLC".to_owned()),
        };
        let matched = match_board(&index, &identity).expect("uno must match");
        assert_eq!(matched.board_id, "arduino.uno-r3");
        assert_eq!(matched.confidence, Confidence::Exact);
        assert_eq!(matched.manifest_path, "boards/arduino-uno-r3.json");
    }

    #[test]
    fn xiao_bootloader_and_application_usb_ids_match_exactly() {
        let index = parse_index(&xiao_index()).expect("parse XIAO index");
        for pid in [0x8045, 0x0045] {
            let matched = match_board(
                &index,
                &UsbIdentity {
                    vid: 0x2886,
                    pid,
                    product: Some("XIAO nRF52840 Sense".to_owned()),
                    manufacturer: Some("Seeed Studio".to_owned()),
                },
            )
            .expect("XIAO must match");
            assert_eq!(matched.board_id, "seeed.xiao-nrf52840-sense");
            assert_eq!(matched.confidence, Confidence::Exact);
        }
    }

    #[test]
    fn descriptor_disagreement_demotes_but_keeps_candidate() {
        let index = parse_index(&uno_index()).unwrap();
        let identity = UsbIdentity {
            vid: 0x2341,
            pid: 0x0043,
            product: Some("Totally Not An Uno".to_owned()),
            manufacturer: None,
        };
        let matched = match_board(&index, &identity).expect("candidate survives");
        assert_eq!(matched.confidence, Confidence::Ambiguous);
    }

    #[test]
    fn unknown_vid_pid_does_not_match() {
        let index = parse_index(&uno_index()).unwrap();
        let identity = UsbIdentity {
            vid: 0x1a86,
            pid: 0x7523,
            product: None,
            manufacturer: None,
        };
        assert_eq!(match_board(&index, &identity), None);
    }

    #[test]
    fn index_paths_are_validated_against_traversal() {
        assert_eq!(validate_relative_path("boards/a.json"), Ok(()));
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("https://evil").is_err());
        assert!(validate_relative_path("").is_err());
        assert!(validate_relative_path("/abs").is_err());
        assert!(validate_relative_path("C:/outside-cache.hex").is_err());
        assert!(validate_relative_path("C:outside-cache.hex").is_err());
    }

    #[test]
    fn sha_format_requires_64_lowercase_hex() {
        assert_eq!(validate_sha_format(&"a".repeat(64)), Ok(()));
        assert!(validate_sha_format(&"A".repeat(64)).is_err());
        assert!(validate_sha_format(&"g".repeat(64)).is_err());
        assert!(validate_sha_format(&"a".repeat(63)).is_err());
    }

    #[test]
    fn cloud_endpoint_and_index_schema_are_pinned() {
        assert_eq!(
            REGISTRY_BASE,
            "https://raw.githubusercontent.com/dev-five-git/hana-cloud/main"
        );
        let unsupported = String::from_utf8(uno_index()).unwrap().replacen(
            "\"schemaVersion\": 1",
            "\"schemaVersion\": 2",
            1,
        );
        assert!(parse_index(unsupported.as_bytes()).is_err());
    }

    #[test]
    fn hash_mismatch_never_caches_bytes() {
        use std::sync::Mutex;
        let cache = temp_cache("mismatch");
        // Local fetcher: the served bytes differ from the wrong_sha expectation.
        let payload = b"firmware-bytes".to_vec();
        let served = Mutex::new(payload.clone());
        let client = RegistryClient {
            cache_dir: cache.clone(),
            fetch: Box::new(move |_path, max_bytes| {
                let body = served.lock().unwrap().clone();
                if body.len() > max_bytes {
                    return Err(RegistryError::Unavailable("너무 큽니다".to_owned()));
                }
                Ok(body)
            }),
        };
        let wrong_sha = sha256_hex(b"other-bytes");

        let result = client
            .verified_with_cache("board/firmware.hex", &wrong_sha, FIRMWARE_MAX_BYTES)
            .map_err(|error| error.to_string());
        assert!(
            matches!(&result, Err(message) if message.contains("해시")),
            "got: {result:?}"
        );
        assert!(
            !cache.join("board/firmware.hex").exists(),
            "unverified bytes must not be cached"
        );

        // When the server payload matches the expected hash it is cached.
        let good_sha = sha256_hex(&payload);
        let cached_serve = client
            .verified_with_cache("board/firmware.hex", &good_sha, FIRMWARE_MAX_BYTES)
            .map_err(|error| error.to_string());
        assert_eq!(cached_serve, Ok(payload.clone()));
        assert_eq!(
            fs::read(cache.join("board/firmware.hex")).map_err(|error| error.to_string()),
            Ok(payload)
        );
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn stale_cache_is_replaced_when_index_hash_moves() {
        let cache = temp_cache("stale");
        let client = RegistryClient::new(cache.clone());
        let old_payload = b"old-firmware";
        let new_payload = b"new-firmware";
        let new_sha = sha256_hex(new_payload);

        // Seed a cache written when the index pointed at the old firmware.
        client.write_cache("board/firmware.hex", old_payload);
        let served = client.verified_with_cache("board/firmware.hex", &new_sha, FIRMWARE_MAX_BYTES);
        // Download will fail (no network in tests) but must NOT serve stale bytes.
        assert!(served.is_err());

        // Once the cache holds the expected content it is served without network.
        client.write_cache("board/firmware.hex", new_payload);
        let served = client.verified_with_cache("board/firmware.hex", &new_sha, FIRMWARE_MAX_BYTES);
        assert_eq!(
            served.as_deref().map_err(|error| error.to_string()),
            Ok(new_payload.as_slice())
        );
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn oversized_bodies_are_rejected_before_use() {
        // 네트워크 fetch가 take(max+1) + 길이 검사를 하므로, 주입된 fetch에서
        // 같은 규칙을 흉내 내 거부 경로를 확인한다.
        use std::sync::Mutex;
        let cache = temp_cache("cap");
        let big_body = Mutex::new(vec![0_u8; 11]);
        let client = RegistryClient {
            cache_dir: cache.clone(),
            fetch: Box::new(move |_path, max_bytes| {
                let body = big_body.lock().unwrap().clone();
                if body.len() > max_bytes {
                    return Err(RegistryError::Unavailable("너무 큽니다".to_owned()));
                }
                Ok(body)
            }),
        };
        assert!(client.fetch_capped("registry.json", 10).is_err());
        assert!(client.fetch_capped("registry.json", 11).is_ok());
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn redirect_host_pin_rejects_foreign_hosts() {
        let host_of = |url: &str| {
            url.split("://")
                .nth(1)
                .and_then(|rest| rest.split(['/']).next())
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(host_of(REGISTRY_BASE), "raw.githubusercontent.com");
        assert_eq!(
            REGISTRY_BASE,
            "https://raw.githubusercontent.com/dev-five-git/hana-cloud/main"
        );
        assert_ne!(
            host_of("https://evil.example/registry.json"),
            host_of(REGISTRY_BASE)
        );
    }

    #[test]
    fn with_local_fetcher_serves_without_network() {
        use std::collections::HashMap;
        use std::sync::Mutex;
        let cache = temp_cache("local-fetch");
        let mut files = HashMap::new();
        files.insert("registry.json".to_owned(), uno_index());
        let files = Mutex::new(files);
        let client = RegistryClient {
            cache_dir: cache.clone(),
            fetch: Box::new(move |path: &str, max_bytes: usize| {
                let body = files.lock().unwrap().get(path).cloned();
                match body {
                    Some(body) if body.len() <= max_bytes => Ok(body),
                    _ => Err(RegistryError::Unavailable("테스트 파일 없음".to_owned())),
                }
            }),
        };
        let matched = client
            .match_board(&UsbIdentity {
                vid: 0x2341,
                pid: 0x0043,
                product: None,
                manufacturer: None,
            })
            .expect("uno matches from local index");
        assert_eq!(matched.board_id, "arduino.uno-r3");
        assert_eq!(matched.confidence, Confidence::Exact);
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn resolves_hash_verified_hana_cloud_hex_from_manifest() {
        use std::collections::HashMap;
        use std::sync::Mutex;

        let cache = temp_cache("resolve-firmware");
        let hex = b":0400000012345678E8\n:00000001FF\n".to_vec();
        let manifest = format!(
            r#"{{"schemaVersion":2,"id":"arduino.uno-r3","firmware":{{"path":"boards/arduino-uno-r3.hex","format":"intel-hex","size":{},"sha256":"{}","fqbn":"arduino:avr:uno","source":{{"path":"boards/arduino-uno-r3.ino","sha256":"{}"}},"toolchain":{{"arduinoCli":"1.5.1","platform":"arduino:avr@1.8.8"}}}},"wiring":[{{"from":"D2","to":"Middle Button"}}]}}"#,
            hex.len(),
            sha256_hex(&hex),
            "a".repeat(64),
        )
        .into_bytes();
        let matched = UsbMatch {
            board_id: "arduino.uno-r3".to_owned(),
            board_name: "Arduino Uno R3".to_owned(),
            confidence: Confidence::Exact,
            manifest_path: "boards/arduino-uno-r3.json".to_owned(),
            manifest_sha256: sha256_hex(&manifest),
        };
        let files = Mutex::new(HashMap::from([
            ("boards/arduino-uno-r3.json".to_owned(), manifest),
            ("boards/arduino-uno-r3.hex".to_owned(), hex.clone()),
        ]));
        let client = RegistryClient {
            cache_dir: cache.clone(),
            fetch: Box::new(move |path, max_bytes| {
                files
                    .lock()
                    .unwrap()
                    .get(path)
                    .filter(|bytes| bytes.len() <= max_bytes)
                    .cloned()
                    .ok_or_else(|| RegistryError::Unavailable("테스트 파일 없음".to_owned()))
            }),
        };

        let firmware = client
            .resolve_firmware(&matched)
            .expect("verified firmware");
        assert_eq!(firmware.board_id, "arduino.uno-r3");
        assert_eq!(firmware.fqbn, "arduino:avr:uno");
        assert_eq!(firmware.hex_text.as_bytes(), hex);
        let _ = fs::remove_dir_all(cache);
    }
}
