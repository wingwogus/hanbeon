//! 스위치 입력 수신과 제스처 판정.
//!
//! 스위치는 USB/BT HID 키보드로 인식되고 눌린 동안 지정 키를 유지한다고 가정한다
//! (docs/PRD.md 7절). 전역 단축키로 그 키를 잡으면 대상 앱으로 새어 나가지도 않는다.

use std::time::Instant;

#[allow(unused_imports)] // integration tests include this adapter module directly.
pub use hanbeon_core::gesture::{Gesture, GestureDetector, Judgement, SharedDetector};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
#[cfg(feature = "desktop")]
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

/// 스위치를 누를 때마다 판정 결과를 알린다. 설정 화면이 구독한다.
pub const EVENT_GESTURE: &str = "input://gesture";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GesturePayload {
    gesture: Gesture,
    held_ms: u64,
}

/// 스위치가 보내는 기본 키. 일반 사용에서 충돌이 가장 적다.
#[cfg(feature = "desktop")]
pub const DEFAULT_SWITCH_CODE: Code = Code::F13;

#[cfg(feature = "desktop")]
pub fn parse_code(name: &str) -> Option<Code> {
    name.parse::<Code>().ok()
}

/// 실제로 쓸 스위치 키.
///
/// 환경변수가 프로필을 이긴다. 실기 스위치 없이 검증할 때 저장된 설정을
/// 건드리지 않고 키만 바꿔 끼울 수 있어야 하기 때문이다.
#[cfg(feature = "desktop")]
pub fn configured_code(profile_key: &str) -> Code {
    if let Ok(name) = std::env::var("HANBEON_SWITCH_KEY") {
        return parse_code(&name).unwrap_or_else(|| {
            eprintln!("HANBEON_SWITCH_KEY='{name}'를 해석하지 못해 기본 키를 씁니다.");
            DEFAULT_SWITCH_CODE
        });
    }
    parse_code(profile_key).unwrap_or(DEFAULT_SWITCH_CODE)
}

/// 설정 화면의 스위치 테스트가 이 값을 그대로 보여준다.
/// Native serial and HID/F13 share this so a judgement is visible once.
pub(crate) fn announce(app: &AppHandle, judgement: Judgement) {
    let _ = app.emit(
        EVENT_GESTURE,
        GesturePayload {
            gesture: judgement.gesture,
            held_ms: judgement.held.as_millis() as u64,
        },
    );
}

/// 전역 단축키로 스위치 키를 잡고, 판정된 제스처를 콜백으로 넘긴다.
#[cfg(feature = "desktop")]
pub fn register<F>(
    app: &AppHandle,
    detector: SharedDetector,
    code: Code,
    on_gesture: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(&AppHandle, Judgement) + Send + Sync + 'static,
{
    println!("스위치 키: {code:?}");

    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            // 우리가 등록하는 단축키는 스위치 키 하나뿐이라 무엇이 왔는지
            // 따로 가리지 않는다. 키를 바꿔 끼워도 이 핸들러는 그대로 쓴다.
            .with_handler(move |app, shortcut, event| {
                let now = Instant::now();
                if std::env::var("HANBEON_LOG").is_ok() {
                    eprintln!("[input] {shortcut:?} {:?}", event.state());
                }
                let Ok(mut detector) = detector.lock() else {
                    return;
                };

                match event.state() {
                    ShortcutState::Pressed => detector.on_press(now),
                    ShortcutState::Released => {
                        if let Some(judgement) = detector.on_release(now) {
                            // 락을 쥔 채 키를 주입하면 다음 입력이 막힌다.
                            drop(detector);
                            announce(app, judgement);
                            on_gesture(app, judgement);
                        }
                    }
                }
            })
            .build(),
    )?;

    app.global_shortcut().register(Shortcut::new(None, code))?;
    Ok(())
}

/// 스위치 키를 바꿔 끼운다.
///
/// 새 키 등록에 실패하면 이전 키를 되살린다. 스위치가 아무 키에도 붙어 있지
/// 않은 상태로 두면 사용자는 앱을 조작할 방법을 완전히 잃는다.
#[cfg(feature = "desktop")]
pub fn rebind(app: &AppHandle, old: Code, new: Code) -> Result<(), String> {
    if old == new {
        return Ok(());
    }

    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister(Shortcut::new(None, old));

    if let Err(error) = shortcuts.register(Shortcut::new(None, new)) {
        let _ = shortcuts.register(Shortcut::new(None, old));
        return Err(format!(
            "스위치 키 '{new:?}'를 등록하지 못했습니다. 다른 프로그램이 쓰고 있을 수 있습니다. 이전 키로 되돌렸습니다. ({error})"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hanbeon_core::gesture::GestureDetector;
    use std::time::Duration;

    fn detector() -> GestureDetector {
        GestureDetector::new(Duration::from_millis(600))
    }

    #[test]
    fn 임계값보다_짧게_누르면_짧게_누름이다() {
        let mut d = detector();
        let t0 = Instant::now();
        d.on_press(t0);
        assert_eq!(
            d.on_release(t0 + Duration::from_millis(200))
                .map(|j| j.gesture),
            Some(Gesture::Short)
        );
    }

    #[test]
    fn 임계값_이상_누르면_길게_누름이다() {
        let mut d = detector();
        let t0 = Instant::now();
        d.on_press(t0);
        assert_eq!(
            d.on_release(t0 + Duration::from_millis(600))
                .map(|j| j.gesture),
            Some(Gesture::Long)
        );
    }

    #[test]
    fn 키_리피트가_눌림_시각을_밀지_않는다() {
        let mut d = detector();
        let t0 = Instant::now();
        d.on_press(t0);
        d.on_press(t0 + Duration::from_millis(400));
        d.on_press(t0 + Duration::from_millis(500));
        // 최초 눌림부터 재면 700ms이므로 길게 누름이어야 한다.
        assert_eq!(
            d.on_release(t0 + Duration::from_millis(700))
                .map(|j| j.gesture),
            Some(Gesture::Long)
        );
    }

    #[test]
    fn 뗀_직후의_떨림_입력은_무시한다() {
        let mut d = detector();
        let t0 = Instant::now();
        d.on_press(t0);
        d.on_release(t0 + Duration::from_millis(100));

        d.on_press(t0 + Duration::from_millis(120));
        assert_eq!(d.on_release(t0 + Duration::from_millis(140)), None);
    }

    #[test]
    fn 떨림_시간이_지난_입력은_정상_판정한다() {
        let mut d = detector();
        let t0 = Instant::now();
        d.on_press(t0);
        d.on_release(t0 + Duration::from_millis(100));

        d.on_press(t0 + Duration::from_millis(200));
        assert_eq!(
            d.on_release(t0 + Duration::from_millis(300))
                .map(|j| j.gesture),
            Some(Gesture::Short)
        );
    }

    #[test]
    fn 누름_없는_뗌은_아무것도_아니다() {
        let mut d = detector();
        assert_eq!(d.on_release(Instant::now()), None);
    }

    #[test]
    fn hid_fallback_default_switch_code_remains_f13() {
        assert_eq!(DEFAULT_SWITCH_CODE, Code::F13);
        assert_eq!(parse_code("F13"), Some(Code::F13));
        assert_eq!(parse_code("KeyA"), Some(Code::KeyA));
    }
}
