//! 스위치 입력 수신과 제스처 판정.
//!
//! 스위치는 USB/BT HID 키보드로 인식되고 눌린 동안 지정 키를 유지한다고 가정한다
//! (docs/PRD.md 7절). 전역 단축키로 그 키를 잡으면 대상 앱으로 새어 나가지도 않는다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
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
pub const DEFAULT_SWITCH_CODE: Code = Code::F13;

/// 이 시간 이상 눌려 있으면 길게 누름으로 본다. 설정에서 300~1500ms로 조정한다.
pub const DEFAULT_LONG_PRESS_MS: u64 = 600;

/// 접점 떨림 필터. 뗀 직후 이 시간 안의 누름은 같은 입력으로 본다.
const DEBOUNCE_MS: u64 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Gesture {
    /// 짧게 누름 — 현재 칸 실행
    Short,
    /// 길게 누름 — 취소 / 일시정지
    Long,
}

/// 판정 결과와 근거.
///
/// 설정 화면의 스위치 테스트는 '얼마나 눌렀고 그래서 무엇으로 읽혔는지'를
/// 보여줘야 한다. 임계값을 조정하려면 사용자가 자기 입력을 볼 수 있어야 한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judgement {
    pub gesture: Gesture,
    pub held: Duration,
}

/// 눌림·뗌 시각만으로 제스처를 판정한다.
///
/// 시각을 인자로 받아 시계에 의존하지 않는다. 판정 규칙은 피로에 따라 조정될
/// 값이라 단위 테스트로 고정해 둘 필요가 있다.
pub struct GestureDetector {
    pressed_at: Option<Instant>,
    released_at: Option<Instant>,
    long_press: Duration,
}

impl GestureDetector {
    pub fn new(long_press: Duration) -> Self {
        Self {
            pressed_at: None,
            released_at: None,
            long_press,
        }
    }

    /// 설정 화면에서 임계값을 바꿀 때 쓴다(M3).
    #[allow(dead_code)]
    pub fn set_long_press(&mut self, long_press: Duration) {
        self.long_press = long_press;
    }

    pub fn on_press(&mut self, now: Instant) {
        // 키 리피트로 눌림이 반복 전달돼도 최초 시각을 유지한다.
        if self.pressed_at.is_some() {
            return;
        }
        // 접점 떨림으로 들어온 재입력은 무시한다.
        if let Some(released) = self.released_at
            && now.duration_since(released) < Duration::from_millis(DEBOUNCE_MS)
        {
            return;
        }
        self.pressed_at = Some(now);
    }

    pub fn on_release(&mut self, now: Instant) -> Option<Judgement> {
        let pressed_at = self.pressed_at.take()?;
        self.released_at = Some(now);

        let held = now.duration_since(pressed_at);
        Some(Judgement {
            gesture: if held >= self.long_press {
                Gesture::Long
            } else {
                Gesture::Short
            },
            held,
        })
    }
}

impl Default for GestureDetector {
    fn default() -> Self {
        Self::new(Duration::from_millis(DEFAULT_LONG_PRESS_MS))
    }
}

/// 여러 곳(설정 저장, 스위치 테스트)에서 임계값을 바꿔야 해서 공유한다.
pub type SharedDetector = Arc<Mutex<GestureDetector>>;

pub fn parse_code(name: &str) -> Option<Code> {
    name.parse::<Code>().ok()
}

/// 실제로 쓸 스위치 키.
///
/// 환경변수가 프로필을 이긴다. 실기 스위치 없이 검증할 때 저장된 설정을
/// 건드리지 않고 키만 바꿔 끼울 수 있어야 하기 때문이다.
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
