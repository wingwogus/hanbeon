//! 스위치 눌림 시간으로 짧게/길게를 가른다.
//!
//! 시각을 인자로 받아 시계에 의존하지 않는다. 판정 규칙은 피로에 따라 조정될
//! 값이라 단위 테스트로 고정해 둘 필요가 있다.
//!
//! 두 플랫폼이 같은 판정을 쓴다. 미묘하게 달라지면 사용자가 기기를 옮길 때마다
//! 누르는 법을 다시 익혀야 한다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// 짧게/길게를 가르는 기본 임계값.
pub const DEFAULT_LONG_PRESS_MS: u64 = 600;

/// 접점 떨림으로 들어온 재입력을 무시하는 시간.
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
}
