//! 지금 앞에 있는 앱을 지켜보는 하나의 루프.
//!
//! 두 가지를 이 한 곳에서 본다. 앱이 바뀌면 스캔 대상을 갈아 끼우고(`preset`),
//! 조작할 요소를 우리가 가리면 창을 반투명하게 한다(`occlusion`). 둘 다 같은
//! 값(활성 앱)에서 출발하므로 루프를 나누면 같은 것을 두 번 묻게 된다.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::app_registry::Registry;
use crate::focused_application::{self, FocusedApplication};
use crate::occlusion::{self, MARGIN};
use hanbeon_core::profile::Profile;
use hanbeon_core::scan::Scanner;

/// 가림 여부가 바뀔 때만 프론트로 보낸다.
pub const EVENT_COVER: &str = "window://cover";

/// 확인 주기.
///
/// 주사 간격(최소 800ms)보다 짧아 커서가 한 칸 머무는 동안 최소 두 번은 본다.
/// 더 자주 보면 접근성 API 호출이 늘어 대상 앱이 느려진다.
const POLL: Duration = Duration::from_millis(300);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoverPayload {
    covered: bool,
    /// 가릴 때 쓸 불투명도(퍼센트). 프론트가 프로필을 따로 읽지 않아도 되게
    /// 함께 보낸다 — 설정을 바꾸는 즉시 반영되어야 한다.
    percent: u8,
}

fn log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("HANBEON_LOG").is_ok())
}

pub fn watch(app: AppHandle, profile: Arc<Mutex<Profile>>, scanner: Scanner, registry: Registry) {
    thread::spawn(move || {
        let mut covered = false;
        let mut source = focused_application::system_source();

        loop {
            thread::sleep(POLL);

            let front = source.current();

            // 우리 자신이 앞에 있는 경우(설정 창을 열었을 때)는 둘 다 건너뛴다.
            // 포커스 요소로 우리 웹뷰가 잡혀 창이 자기 자신을 가린다고 판정되고,
            // 스캔 대상도 설정 창을 여닫을 때마다 두 번씩 바뀐다.
            let ours = is_ours(front.as_ref(), std::process::id() as i32);

            if !ours {
                let enabled = profile
                    .lock()
                    .map(|profile| profile.app_buttons)
                    .unwrap_or(false);
                let preset = if enabled {
                    front
                        .as_ref()
                        .and_then(|focused| registry.lookup(focused))
                        .and_then(crate::preset::PresetSelection::from_registry)
                } else {
                    None
                };
                let key = preset.as_ref().map(|preset| preset.key.clone());
                let registry_id = preset.as_ref().map(|preset| preset.registry_id.clone());
                let name = preset.as_ref().map(|preset| preset.name.clone());
                let cells = crate::preset::cells_for(preset.as_ref());
                scanner.apply_cells(key, registry_id, name, cells);
            }

            let (dim, percent) = profile
                .lock()
                .map(|profile| (profile.dim_when_covered, profile.dim_percent))
                .unwrap_or((false, 100));

            let now = if dim && !ours {
                let window = app
                    .get_webview_window("floating")
                    .as_ref()
                    .and_then(occlusion::window_rect);
                let element = front
                    .as_ref()
                    .and_then(FocusedApplication::pid)
                    .and_then(occlusion::focused_element_rect);

                let verdict = match (window, element) {
                    (Some(window), Some(element)) => window.overlaps(&element, MARGIN),
                    // 창이나 요소 위치를 모르면 가리지 않은 것으로 둔다.
                    _ => false,
                };

                if log_enabled() {
                    eprintln!("[cover] 창 {window:?} 요소 {element:?} -> {verdict}");
                }

                verdict
            } else {
                false
            };

            // 매 주기마다 보내지 않는다. 바뀔 때만 보내야 프론트가 같은 값으로
            // 다시 그리지 않고, 로그도 읽을 수 있는 양으로 남는다.
            if now != covered {
                covered = now;
                let _ = app.emit(EVENT_COVER, CoverPayload { covered, percent });
            }
        }
    });
}

fn is_ours(front: Option<&FocusedApplication>, own_pid: i32) -> bool {
    front.and_then(FocusedApplication::pid) == Some(own_pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focused_application::FocusedApplication;

    #[test]
    fn own_focused_application_is_ignored() {
        let front = FocusedApplication::macos(42, Some("devfive.hanbeon".into()));
        assert!(is_ours(Some(&front), 42));
        assert!(!is_ours(Some(&front), 7));
        assert!(!is_ours(None, 42));
    }
}
