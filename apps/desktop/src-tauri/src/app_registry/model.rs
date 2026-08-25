use std::collections::HashSet;

use serde::Deserialize;

use crate::focused_application::{ApplicationIdentity, FocusedApplication};

pub(crate) const INDEX_LIMIT: usize = 256 * 1024;
pub(crate) const PROFILE_LIMIT: usize = 64 * 1024;
const MAX_APPS: usize = 1024;
const MAX_ALIASES: usize = 32;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegistryIndex {
    schema_version: u8,
    pub(crate) revision: u64,
    pub(crate) apps: Vec<AppEntry>,
    boards: Vec<BoardEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) sha256: String,
    #[serde(rename = "match")]
    matchers: AppMatchers,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppMatchers {
    macos: Option<MacOsMatchers>,
    windows: Option<WindowsMatchers>,
    linux: Option<LinuxMatchers>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MacOsMatchers {
    bundle_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowsMatchers {
    executables: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinuxMatchers {
    desktop_ids: Vec<String>,
    wm_classes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BoardEntry {
    id: String,
    name: String,
    manifest: String,
    sha256: String,
    detect: BoardDetection,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BoardDetection {
    usb: Vec<UsbMatch>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsbMatch {
    vid: String,
    pid: String,
    confidence: UsbConfidence,
    #[serde(default)]
    manufacturer_aliases: Vec<String>,
    #[serde(default)]
    product_aliases: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum UsbConfidence {
    Exact,
    Likely,
    Ambiguous,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppProfile {
    schema_version: u8,
    pub(crate) id: String,
    actions: Vec<ProfileAction>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileAction {
    label: String,
    name: String,
    shortcut: PlatformShortcuts,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlatformShortcuts {
    macos: Option<String>,
    windows: Option<String>,
    linux: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // 각 빌드는 현재 OS 외 variant를 만들지 않지만 검증 로직은 공유한다.
pub(crate) enum Platform {
    MacOs,
    Windows,
    Linux,
}

impl Platform {
    pub(crate) fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self::Linux
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedAction {
    pub(crate) label: String,
    pub(crate) name: String,
    pub(crate) shortcut: String,
}

impl AppProfile {
    pub(crate) fn actions_for(&self, platform: Platform) -> Vec<ResolvedAction> {
        self.actions
            .iter()
            .filter_map(|action| {
                let shortcut = match platform {
                    Platform::MacOs => action.shortcut.macos.as_ref(),
                    Platform::Windows => action.shortcut.windows.as_ref(),
                    Platform::Linux => action.shortcut.linux.as_ref(),
                }?;
                Some(ResolvedAction {
                    label: action.label.clone(),
                    name: action.name.clone(),
                    shortcut: shortcut.clone(),
                })
            })
            .collect()
    }
}

pub(crate) fn parse_index(raw: &[u8]) -> Result<RegistryIndex, String> {
    if raw.len() > INDEX_LIMIT {
        return Err(format!(
            "registry가 {}바이트 제한을 넘었습니다.",
            INDEX_LIMIT
        ));
    }

    let index: RegistryIndex = serde_json::from_slice(raw)
        .map_err(|error| format!("registry JSON을 읽지 못했습니다. ({error})"))?;
    validate_index(&index)?;
    Ok(index)
}

pub(crate) fn parse_profile(raw: &[u8], expected_id: &str) -> Result<AppProfile, String> {
    if raw.len() > PROFILE_LIMIT {
        return Err(format!(
            "app profile이 {}바이트 제한을 넘었습니다.",
            PROFILE_LIMIT
        ));
    }

    let profile: AppProfile = serde_json::from_slice(raw)
        .map_err(|error| format!("app profile JSON을 읽지 못했습니다. ({error})"))?;
    validate_profile(&profile, expected_id)?;
    Ok(profile)
}

fn validate_index(index: &RegistryIndex) -> Result<(), String> {
    if index.schema_version != 1 {
        return Err("registry schemaVersion은 1이어야 합니다.".into());
    }
    if index.revision == 0 {
        return Err("registry revision은 1 이상이어야 합니다.".into());
    }
    if index.apps.len() > MAX_APPS {
        return Err(format!("registry apps는 {MAX_APPS}개를 넘을 수 없습니다."));
    }
    let mut ids = HashSet::new();
    let mut bundle_ids = HashSet::new();
    let mut executables = HashSet::new();
    let mut desktop_ids = HashSet::new();
    let mut wm_classes = HashSet::new();

    for entry in &index.apps {
        if !valid_id(&entry.id) || !ids.insert(entry.id.as_str()) {
            return Err(format!("app id가 잘못됐거나 중복됐습니다. ({})", entry.id));
        }
        bounded_text(&entry.name, 100, "app name")?;
        if !safe_app_path(&entry.path) {
            return Err(format!("app path가 안전하지 않습니다. ({})", entry.path));
        }
        if !valid_sha256(&entry.sha256) {
            return Err(format!("app sha256이 잘못됐습니다. ({})", entry.id));
        }

        let mut platforms = 0;
        if let Some(macos) = &entry.matchers.macos {
            platforms += 1;
            validate_aliases(&macos.bundle_ids, valid_bundle_id, "macOS bundleIds")?;
            for value in &macos.bundle_ids {
                if !bundle_ids.insert(value.as_str()) {
                    return Err(format!("macOS bundleId가 중복됐습니다. ({value})"));
                }
            }
        }
        if let Some(windows) = &entry.matchers.windows {
            platforms += 1;
            validate_aliases(
                &windows.executables,
                valid_windows_executable,
                "Windows executables",
            )?;
            for value in &windows.executables {
                let normalized = value.to_lowercase();
                if !executables.insert(normalized) {
                    return Err(format!("Windows executable이 중복됐습니다. ({value})"));
                }
            }
        }
        if let Some(linux) = &entry.matchers.linux {
            platforms += 1;
            validate_aliases(&linux.desktop_ids, valid_desktop_id, "Linux desktopIds")?;
            validate_aliases(&linux.wm_classes, valid_wm_class, "Linux wmClasses")?;
            for value in &linux.desktop_ids {
                if !desktop_ids.insert(value.as_str()) {
                    return Err(format!("Linux desktopId가 중복됐습니다. ({value})"));
                }
            }
            for value in &linux.wm_classes {
                if !wm_classes.insert(value.as_str()) {
                    return Err(format!("Linux WM_CLASS가 중복됐습니다. ({value})"));
                }
            }
        }
        if platforms == 0 {
            return Err(format!("app match가 비어 있습니다. ({})", entry.id));
        }
    }

    let mut board_ids = HashSet::new();
    for entry in &index.boards {
        if !valid_id(&entry.id) || !board_ids.insert(entry.id.as_str()) {
            return Err(format!(
                "board id가 잘못됐거나 중복됐습니다. ({})",
                entry.id
            ));
        }
        bounded_text(&entry.name, 100, "board name")?;
        if !safe_board_path(&entry.manifest) {
            return Err(format!(
                "board manifest 경로가 안전하지 않습니다. ({})",
                entry.manifest
            ));
        }
        if !valid_sha256(&entry.sha256) {
            return Err(format!("board sha256이 잘못됐습니다. ({})", entry.id));
        }
        if entry.detect.usb.is_empty() {
            return Err(format!("board detect.usb가 비어 있습니다. ({})", entry.id));
        }
        for matcher in &entry.detect.usb {
            if !valid_usb_id(&matcher.vid) || !valid_usb_id(&matcher.pid) {
                return Err(format!(
                    "board USB VID/PID가 잘못됐습니다. ({}:{})",
                    matcher.vid, matcher.pid
                ));
            }
            validate_usb_aliases(&matcher.manufacturer_aliases, "manufacturerAliases")?;
            validate_usb_aliases(&matcher.product_aliases, "productAliases")?;
            match matcher.confidence {
                UsbConfidence::Exact | UsbConfidence::Likely | UsbConfidence::Ambiguous => {}
            }
        }
    }

    Ok(())
}

fn validate_profile(profile: &AppProfile, expected_id: &str) -> Result<(), String> {
    if profile.schema_version != 1 {
        return Err("app profile schemaVersion은 1이어야 합니다.".into());
    }
    if !valid_id(&profile.id) || profile.id != expected_id {
        return Err(format!(
            "app profile id가 registry와 다릅니다. (예상 {expected_id}, 실제 {})",
            profile.id
        ));
    }
    if !(1..=3).contains(&profile.actions.len()) {
        return Err("app profile actions는 1~3개여야 합니다.".into());
    }

    for (index, action) in profile.actions.iter().enumerate() {
        bounded_text(&action.label, 20, "action label")?;
        bounded_text(&action.name, 60, "action name")?;

        let shortcuts = [
            action.shortcut.macos.as_deref(),
            action.shortcut.windows.as_deref(),
            action.shortcut.linux.as_deref(),
        ];
        if shortcuts.iter().all(Option::is_none) {
            return Err(format!("action shortcut이 비어 있습니다. ({index})"));
        }
        for shortcut in shortcuts.into_iter().flatten() {
            if shortcut.is_empty()
                || shortcut.len() > 64
                || shortcut.trim() != shortcut
                || shortcut != shortcut.to_ascii_lowercase()
                || !shortcut
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'+')
                || hanbeon_core::shortcut::parse(shortcut).is_none()
            {
                return Err(format!("지원하지 않는 shortcut입니다. ({shortcut})"));
            }
        }
    }

    for left in 0..profile.actions.len() {
        for right in (left + 1)..profile.actions.len() {
            if profile.actions[left] == profile.actions[right] {
                return Err("동일한 action을 두 번 등록할 수 없습니다.".into());
            }
        }
    }

    Ok(())
}

pub(crate) fn matches_application(entry: &AppEntry, app: &FocusedApplication) -> bool {
    match app.identity() {
        ApplicationIdentity::MacOs { bundle_id } => entry
            .matchers
            .macos
            .as_ref()
            .zip(bundle_id.as_ref())
            .is_some_and(|(aliases, value)| aliases.bundle_ids.contains(value)),
        ApplicationIdentity::Windows { executable } => {
            entry.matchers.windows.as_ref().is_some_and(|aliases| {
                aliases
                    .executables
                    .iter()
                    .any(|value| value.to_lowercase() == executable.to_lowercase())
            })
        }
        ApplicationIdentity::Linux {
            desktop_id,
            wm_classes,
        } => entry.matchers.linux.as_ref().is_some_and(|aliases| {
            desktop_id
                .as_ref()
                .is_some_and(|value| aliases.desktop_ids.contains(value))
                || wm_classes
                    .iter()
                    .any(|value| aliases.wm_classes.contains(value))
        }),
    }
}

fn validate_aliases(
    aliases: &[String],
    validate: fn(&str) -> bool,
    field: &str,
) -> Result<(), String> {
    if aliases.is_empty() || aliases.len() > MAX_ALIASES {
        return Err(format!("{field}는 1~{MAX_ALIASES}개여야 합니다."));
    }
    let mut unique = HashSet::new();
    if aliases
        .iter()
        .any(|value| !validate(value) || !unique.insert(value.as_str()))
    {
        return Err(format!("{field}에 잘못됐거나 중복된 값이 있습니다."));
    }
    Ok(())
}

fn bounded_text(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    let length = value.chars().count();
    if length == 0 || length > maximum {
        return Err(format!("{field} 길이는 1~{maximum}자여야 합니다."));
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    let mut previous_separator = true;
    for byte in value.bytes() {
        if byte == b'.' || byte == b'-' {
            if previous_separator {
                return false;
            }
            previous_separator = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            previous_separator = false;
        } else {
            return false;
        }
    }
    !value.is_empty() && !previous_separator
}

fn safe_app_path(value: &str) -> bool {
    safe_registry_path(value, "apps/", ".json")
}

fn safe_board_path(value: &str) -> bool {
    safe_registry_path(value, "boards/", ".json")
}

fn safe_registry_path(value: &str, prefix: &str, suffix: &str) -> bool {
    value.starts_with(prefix)
        && value.ends_with(suffix)
        && !value.contains('\\')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
}

fn valid_usb_id(value: &str) -> bool {
    value.len() == 4
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_usb_aliases(aliases: &[String], field: &str) -> Result<(), String> {
    let mut unique = HashSet::new();
    for alias in aliases {
        bounded_text(alias, 100, field)?;
        if !unique.insert(alias.as_str()) {
            return Err(format!("board {field}에 중복된 값이 있습니다."));
        }
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_bundle_id(value: &str) -> bool {
    value.len() <= 255
        && value.contains('.')
        && value.split('.').all(|segment| {
            let mut bytes = segment.bytes();
            bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_windows_executable(value: &str) -> bool {
    value.len() <= 255
        && value.len() > 4
        && value[value.len() - 4..].eq_ignore_ascii_case(".exe")
        && value.trim() == value
        && !value.starts_with('.')
        && !value
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
}

fn valid_desktop_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.contains("..")
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn valid_wm_class(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 255
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focused_application::FocusedApplication;

    const PROFILE_HASH: &str = "739fdb18d93b143ffe6598c26177be73794f12052ad6018eca24d1912cbf22a8";

    fn index_json(path: &str) -> String {
        format!(
            r#"{{
                "schemaVersion": 1,
                "revision": 2,
                "apps": [{{
                    "id": "pdf-viewer",
                    "name": "PDF 뷰어",
                    "path": "{path}",
                    "sha256": "{PROFILE_HASH}",
                    "match": {{
                        "macos": {{ "bundleIds": ["com.apple.Preview"] }},
                        "windows": {{ "executables": ["Acrobat.exe"] }},
                        "linux": {{
                            "desktopIds": ["org.gnome.Evince.desktop"],
                            "wmClasses": ["evince", "Evince"]
                        }}
                    }}
                }}],
                "boards": []
            }}"#
        )
    }

    fn profile_json(actions: &str) -> String {
        format!(
            r#"{{
                "schemaVersion": 1,
                "id": "pdf-viewer",
                "actions": [{actions}]
            }}"#
        )
    }

    #[test]
    fn parses_the_published_index_and_matches_each_platform_identity() {
        let index = parse_index(index_json("apps/pdf-viewer.json").as_bytes()).unwrap();
        let entry = &index.apps[0];

        assert_eq!(entry.id, "pdf-viewer");
        assert!(matches_application(
            entry,
            &FocusedApplication::macos(1, Some("com.apple.Preview".into()))
        ));
        assert!(matches_application(
            entry,
            &FocusedApplication::windows(2, "ACROBAT.EXE".into())
        ));
        assert!(matches_application(
            entry,
            &FocusedApplication::linux(
                Some(3),
                Some("org.gnome.Evince.desktop".into()),
                vec!["unrelated".into()]
            )
        ));
        assert!(matches_application(
            entry,
            &FocusedApplication::linux(None, None, vec!["Evince".into()])
        ));
    }

    #[test]
    fn platform_identifiers_are_not_display_string_normalized() {
        let index = parse_index(index_json("apps/pdf-viewer.json").as_bytes()).unwrap();
        let entry = &index.apps[0];

        assert!(!matches_application(
            entry,
            &FocusedApplication::macos(1, Some("COM.APPLE.PREVIEW".into()))
        ));
        assert!(!matches_application(
            entry,
            &FocusedApplication::linux(None, None, vec!["e-v_i nce".into()])
        ));
    }

    #[test]
    fn rejects_an_unsafe_profile_path() {
        let error = parse_index(index_json("apps/../boards/firmware.ino").as_bytes()).unwrap_err();
        assert!(error.contains("path"));
    }

    #[test]
    fn rejects_an_unknown_index_schema_version() {
        let raw = index_json("apps/pdf-viewer.json")
            .replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        let error = parse_index(raw.as_bytes()).unwrap_err();
        assert!(error.contains("schemaVersion"));
    }

    #[test]
    fn rejects_a_board_without_the_registry_schema_fields() {
        let raw = index_json("apps/pdf-viewer.json").replace("\"boards\": []", "\"boards\": [{}]");

        assert!(parse_index(raw.as_bytes()).is_err());
    }

    #[test]
    fn rejects_invalid_or_unknown_board_usb_fields() {
        let board = format!(
            r#"{{
                "id": "arduino-uno-r3",
                "name": "Arduino Uno R3",
                "manifest": "boards/arduino-uno-r3.json",
                "sha256": "{PROFILE_HASH}",
                "detect": {{
                    "usb": [{{
                        "vid": "2341",
                        "pid": "00ZZ",
                        "confidence": "exact",
                        "serialHandshake": true
                    }}]
                }}
            }}"#
        );
        let raw = index_json("apps/pdf-viewer.json")
            .replace("\"boards\": []", &format!("\"boards\": [{board}]"));

        assert!(parse_index(raw.as_bytes()).is_err());
    }

    #[test]
    fn accepts_a_complete_board_registry_entry() {
        let board = format!(
            r#"{{
                "id": "arduino-uno-r3",
                "name": "Arduino Uno R3",
                "manifest": "boards/arduino-uno-r3.json",
                "sha256": "{PROFILE_HASH}",
                "detect": {{
                    "usb": [{{
                        "vid": "2341",
                        "pid": "0043",
                        "confidence": "exact",
                        "manufacturerAliases": ["Arduino LLC"],
                        "productAliases": ["Arduino Uno"]
                    }}]
                }}
            }}"#
        );
        let raw = index_json("apps/pdf-viewer.json")
            .replace("\"boards\": []", &format!("\"boards\": [{board}]"));

        let index = parse_index(raw.as_bytes()).unwrap();
        assert_eq!(index.boards.len(), 1);
    }

    #[test]
    fn parses_actions_for_each_platform_in_registry_order() {
        let raw = profile_json(
            r#"{
                "label": "다음 장",
                "name": "페이지 넘기기",
                "shortcut": {
                    "macos": "pagedown",
                    "windows": "pagedown",
                    "linux": "pagedown"
                }
            }, {
                "label": "이전 장",
                "name": "앞 페이지로",
                "shortcut": {
                    "macos": "pageup",
                    "windows": "pageup"
                }
            }"#,
        );
        let profile = parse_profile(raw.as_bytes(), "pdf-viewer").unwrap();

        let windows = profile.actions_for(Platform::Windows);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "다음 장");
        assert_eq!(windows[1].shortcut, "pageup");

        let linux = profile.actions_for(Platform::Linux);
        assert_eq!(linux.len(), 1);
        assert_eq!(linux[0].shortcut, "pagedown");
    }

    #[test]
    fn rejects_a_profile_with_the_wrong_id() {
        let raw = profile_json(
            r#"{
                "label": "다음 장",
                "name": "페이지 넘기기",
                "shortcut": { "macos": "pagedown" }
            }"#,
        );
        let error = parse_profile(raw.as_bytes(), "music-app").unwrap_err();
        assert!(error.contains("id"));
    }

    #[test]
    fn rejects_more_than_three_actions_or_an_unknown_shortcut() {
        let action = r#"{
            "label": "동작",
            "name": "테스트",
            "shortcut": { "macos": "pagedown" }
        }"#;
        let too_many = profile_json(&[action, action, action, action].join(","));
        assert!(parse_profile(too_many.as_bytes(), "pdf-viewer").is_err());

        let unknown = profile_json(
            r#"{
                "label": "동작",
                "name": "테스트",
                "shortcut": { "macos": "shell+command" }
            }"#,
        );
        assert!(parse_profile(unknown.as_bytes(), "pdf-viewer").is_err());
    }

    #[test]
    fn enforces_index_and_profile_size_limits_before_parsing() {
        assert!(parse_index(&vec![b' '; INDEX_LIMIT + 1]).is_err());
        assert!(parse_profile(&vec![b' '; PROFILE_LIMIT + 1], "pdf-viewer").is_err());
    }
}
