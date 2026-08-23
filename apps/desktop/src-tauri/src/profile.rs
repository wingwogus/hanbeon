//! 사용자 프로필.
//!
//! 환경이 바뀌어도(오전/오후, 책상/침상) 익숙한 조작감을 유지해야 하므로
//! 모든 조정값을 한 곳에 모아 저장한다. 연결이 끊기거나 앱이 비정상 종료돼도
//! 마지막 저장 설정으로 복구된다(PRD F9).

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// 되돌리기를 어떤 키로 보낼지.
///
/// 어느 쪽이 나은지는 실증에서 정한다(PRD 13절). 기본은 뒤로가기다.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoMapping {
    /// macOS `Cmd+[` / 그 외 `Alt+←`
    #[default]
    Back,
    /// `Cmd/Ctrl+Z`
    Undo,
}

/// `serde(default)`가 필드 단위로 걸려 있다. 설정 항목이 늘어나면 이전 버전이
/// 남긴 파일에는 그 필드가 없는데, 그걸 파싱 실패로 처리하면 사용자가 맞춰 둔
/// 속도·스위치 키가 통째로 기본값으로 되돌아간다. 없는 항목만 기본값을 쓰고
/// 나머지는 지킨다.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    /// 현재 주사 간격.
    pub interval_ms: u64,
    /// 적응 로직이 넘을 수 없는 하한.
    pub min_interval_ms: u64,
    /// 적응 로직이 넘을 수 없는 상한.
    pub max_interval_ms: u64,
    /// 최근 기록을 보고 간격을 조정할지.
    pub adaptive: bool,
    /// 수동 고정. 켜져 있으면 어떤 근거로도 간격을 바꾸지 않는다.
    pub manual_lock: bool,
    /// 이 시간 이상 누르면 길게 누름.
    pub long_press_ms: u64,
    /// 스위치가 보내는 키(`KeyboardEvent.code` 표기).
    pub switch_key: String,
    /// 청각 피드백.
    pub sound: bool,
    pub undo_mapping: UndoMapping,
    /// `light` | `dark` | `contrast`
    pub theme: String,
    /// 사용자가 옮긴 floating 창 위치.
    pub window_position: Option<(i32, i32)>,
    /// 조작할 요소를 컨트롤러가 가릴 때 창을 반투명하게 만들지.
    pub dim_when_covered: bool,
    /// 가릴 때의 불투명도(퍼센트, 25~100). 낮을수록 뒤가 잘 보이고 컨트롤러는 흐려진다.
    pub dim_percent: u8,
    /// 실증용 이벤트 기록을 남길지. 기록은 이 기기 안에만 저장된다.
    pub logging: bool,
    /// 앱에 따라 칸을 더 붙일지. 꺼 두면 어떤 앱에서도 앞 4칸만 돈다.
    pub app_buttons: bool,
    /// 초기 설정 3단계를 마쳤는지. 아니면 첫 실행 안내를 띄운다.
    pub onboarded: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            interval_ms: 1800,
            min_interval_ms: 800,
            max_interval_ms: 4000,
            adaptive: true,
            manual_lock: false,
            long_press_ms: 600,
            switch_key: "F13".to_string(),
            sound: true,
            undo_mapping: UndoMapping::Back,
            theme: "light".to_string(),
            window_position: None,
            dim_when_covered: true,
            // 뒤를 알아볼 수 있으면서 컨트롤러 글자도 남는 지점. 저시력
            // 사용자에게 더 낮은 값은 화면을 잃는 것과 같아서 기본으로 두지 않는다.
            dim_percent: 40,
            logging: true,
            app_buttons: true,
            onboarded: false,
        }
    }
}

impl Profile {
    /// 저장 전에 값을 성립 가능한 범위로 되돌린다.
    ///
    /// 설정 화면이 막아주더라도, 손상된 파일이나 옛 버전이 남긴 값이
    /// 그대로 들어와 스캔이 멈추거나 폭주하게 두면 안 된다.
    pub fn sanitize(&mut self) {
        self.min_interval_ms = self.min_interval_ms.clamp(300, 10_000);
        self.max_interval_ms = self.max_interval_ms.clamp(self.min_interval_ms, 10_000);
        self.interval_ms = self
            .interval_ms
            .clamp(self.min_interval_ms, self.max_interval_ms);
        self.long_press_ms = self.long_press_ms.clamp(300, 1500);

        // 0에 가까운 값이 들어오면 컨트롤러가 통째로 사라진다. 스위치만 쓰는
        // 사용자는 보이지 않는 컨트롤러를 되돌릴 방법이 없다.
        self.dim_percent = self.dim_percent.clamp(25, 100);

        if self.switch_key.trim().is_empty() {
            self.switch_key = "F13".to_string();
        }
        if !matches!(self.theme.as_str(), "light" | "dark" | "contrast") {
            self.theme = "light".to_string();
        }
    }

    /// 적응 로직이 지금 간격을 건드려도 되는지.
    /// 수동 고정이 항상 우선한다(PRD 원칙 1).
    pub fn adaptation_active(&self) -> bool {
        self.adaptive && !self.manual_lock
    }

    fn path(app: &AppHandle) -> Result<PathBuf, String> {
        let dir = app
            .path()
            .app_config_dir()
            .map_err(|e| format!("설정 폴더를 찾지 못했습니다. ({e})"))?;
        Ok(dir.join("profile.json"))
    }

    /// 저장된 프로필을 읽는다.
    ///
    /// 파일이 없거나 깨졌으면 기본값으로 시작한다. 설정을 읽지 못했다고 해서
    /// 앱이 뜨지 않으면 사용자는 스위치 외의 입력 수단이 없어 아무것도 못 한다.
    pub fn load(app: &AppHandle) -> Self {
        let mut profile = Self::path(app)
            .ok()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|raw| match serde_json::from_str::<Profile>(&raw) {
                Ok(profile) => Some(profile),
                Err(error) => {
                    eprintln!("프로필을 읽지 못해 기본값으로 시작합니다. ({error})");
                    None
                }
            })
            .unwrap_or_default();

        profile.sanitize();
        profile
    }

    pub fn save(&self, app: &AppHandle) -> Result<(), String> {
        let path = Self::path(app)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("설정 폴더를 만들지 못했습니다. ({e})"))?;
        }

        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| format!("설정을 변환하지 못했습니다. ({e})"))?;
        fs::write(&path, raw).map_err(|e| format!("설정을 저장하지 못했습니다. ({e})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기본값은_그대로_통과한다() {
        let mut profile = Profile::default();
        let before = profile.clone();
        profile.sanitize();
        assert_eq!(profile, before);
    }

    #[test]
    fn 뒤집힌_범위를_바로잡는다() {
        let mut profile = Profile {
            min_interval_ms: 3000,
            max_interval_ms: 1000,
            interval_ms: 5000,
            ..Default::default()
        };
        profile.sanitize();

        assert!(profile.min_interval_ms <= profile.max_interval_ms);
        assert!(profile.interval_ms >= profile.min_interval_ms);
        assert!(profile.interval_ms <= profile.max_interval_ms);
    }

    #[test]
    fn 간격_0은_스캔을_멈추지_못하게_막는다() {
        let mut profile = Profile {
            interval_ms: 0,
            min_interval_ms: 0,
            ..Default::default()
        };
        profile.sanitize();
        assert!(profile.interval_ms >= 300);
    }

    #[test]
    fn 컨트롤러가_사라질_만큼_투명해지지_않는다() {
        let mut profile = Profile {
            dim_percent: 0,
            ..Default::default()
        };
        profile.sanitize();
        assert!(profile.dim_percent >= 25);
    }

    #[test]
    fn 옛_버전_파일에_없는_항목만_기본값을_쓴다() {
        // 설정 항목이 늘기 전에 저장된 파일. 없는 항목 때문에 파싱이 통째로
        // 실패하면 사용자가 맞춰 둔 속도와 스위치 키가 함께 날아간다.
        let old = r#"{
            "intervalMs": 2200,
            "minIntervalMs": 900,
            "maxIntervalMs": 3800,
            "adaptive": false,
            "manualLock": true,
            "longPressMs": 700,
            "switchKey": "F14",
            "sound": false,
            "undoMapping": "undo",
            "theme": "contrast",
            "windowPosition": [100, 200],
            "onboarded": true
        }"#;

        let profile: Profile = serde_json::from_str(old).expect("옛 파일을 읽어야 한다");

        assert_eq!(profile.interval_ms, 2200);
        assert_eq!(profile.switch_key, "F14");
        assert_eq!(profile.theme, "contrast");
        assert!(profile.manual_lock);

        // 새로 생긴 항목만 기본값.
        assert!(profile.dim_when_covered);
        assert_eq!(profile.dim_percent, 40);
    }

    #[test]
    fn 알_수_없는_테마는_기본값이_된다() {
        let mut profile = Profile {
            theme: "네온".to_string(),
            ..Default::default()
        };
        profile.sanitize();
        assert_eq!(profile.theme, "light");
    }

    #[test]
    fn 수동_고정은_적응보다_우선한다() {
        let profile = Profile {
            adaptive: true,
            manual_lock: true,
            ..Default::default()
        };
        assert!(!profile.adaptation_active());
    }

    #[test]
    fn 적응이_꺼져_있으면_조정하지_않는다() {
        let profile = Profile {
            adaptive: false,
            manual_lock: false,
            ..Default::default()
        };
        assert!(!profile.adaptation_active());
    }
}
