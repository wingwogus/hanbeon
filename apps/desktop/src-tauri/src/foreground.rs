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

use crate::occlusion::{self, MARGIN};
use crate::profile::Profile;
use crate::scan::Scanner;

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

/// 지금 앞에 있는 앱.
struct Frontmost {
    pid: i32,
    bundle_id: Option<String>,
}

#[cfg(target_os = "macos")]
fn frontmost() -> Option<Frontmost> {
    use objc2_app_kit::NSWorkspace;

    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(Frontmost {
        pid: app.processIdentifier(),
        bundle_id: app.bundleIdentifier().map(|id| id.to_string()),
    })
}

/// Windows는 아직 활성 앱을 읽지 않는다. 앱별 칸이 붙지 않고 가림 판정도
/// 하지 않을 뿐, 앞 4칸은 그대로 동작한다.
#[cfg(not(target_os = "macos"))]
fn frontmost() -> Option<Frontmost> {
    None
}

fn log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("HANBEON_LOG").is_ok())
}

pub fn watch(app: AppHandle, profile: Arc<Mutex<Profile>>, scanner: Scanner) {
    thread::spawn(move || {
        let mut covered = false;

        loop {
            thread::sleep(POLL);

            let front = frontmost();

            // 우리 자신이 앞에 있는 경우(설정 창을 열었을 때)는 둘 다 건너뛴다.
            // 포커스 요소로 우리 웹뷰가 잡혀 창이 자기 자신을 가린다고 판정되고,
            // 스캔 대상도 설정 창을 여닫을 때마다 두 번씩 바뀐다.
            let ours = front
                .as_ref()
                .is_some_and(|front| front.pid == std::process::id() as i32);

            if !ours {
                scanner.apply_preset(
                    &app,
                    front.as_ref().and_then(|front| front.bundle_id.as_deref()),
                );
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
                    .and_then(|front| occlusion::focused_element_rect(front.pid));

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
