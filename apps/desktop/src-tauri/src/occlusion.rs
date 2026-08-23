//! 컨트롤러가 대상 앱의 조작 지점을 가리는지 살핀다.
//!
//! floating 창은 항상 맨 앞에 뜬다. 그 아래에 지금 조작해야 할 요소가 들어가면
//! 사용자는 자기가 무엇을 고르고 있는지 볼 수 없다. 커서는 정상적으로 돌고
//! 있으므로 화면만 봐서는 무엇이 잘못됐는지도 알 수 없다.
//!
//! 그래서 '지금 포커스를 가진 요소'의 화면 위치를 읽어 우리 창과 겹치는지 본다.
//! 겹치면 프론트가 창을 반투명하게 만들어 뒤가 보이게 한다.
//!
//! 겹침 판정은 순수 계산(`Rect`)으로 분리했다. 좌표를 읽어 오는 쪽은 플랫폼마다
//! 다르고 실기 없이는 검증할 수 없지만, 판정 규칙은 단위 테스트로 고정할 수 있다.

/// 겹침으로 보기 시작하는 여유(논리 px).
///
/// 딱 붙어 있기만 해도 글자는 이미 읽기 어렵다. 요소 테두리에서 이만큼
/// 떨어져 있어야 가리지 않는 것으로 본다.
pub const MARGIN: f64 = 12.0;

/// 화면 좌표계의 사각형(논리 px, 원점은 주 화면 좌상단).
///
/// 접근성 API가 주는 단위와 같다. 창 좌표는 물리 px로 오므로 배율로 나눠
/// 여기에 맞춘 뒤에 비교한다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// 두 사각형이 여유를 포함해 겹치는지.
    pub fn overlaps(&self, other: &Rect, margin: f64) -> bool {
        // 크기가 없는 요소는 화면에 없는 것과 같다. 접근성 API는 숨겨진
        // 요소에도 0 크기를 돌려주는 일이 있어서, 이걸 겹침으로 보면
        // 창이 이유 없이 흐려진다.
        if self.width <= 0.0 || self.height <= 0.0 {
            return false;
        }
        if other.width <= 0.0 || other.height <= 0.0 {
            return false;
        }

        let apart_x =
            self.x >= other.x + other.width + margin || other.x >= self.x + self.width + margin;
        let apart_y =
            self.y >= other.y + other.height + margin || other.y >= self.y + self.height + margin;

        !(apart_x || apart_y)
    }
}

/// 지금 포커스를 가진 요소의 화면 위치. 알 수 없으면 `None`.
///
/// 알 수 없을 때 가려졌다고 단정하지 않는다. 근거 없이 창을 흐리게 만들면
/// 저시력 사용자는 아무 이유 없이 화면을 잃는다.
#[cfg(target_os = "macos")]
pub fn focused_element_rect(pid: i32) -> Option<Rect> {
    use accessibility::{AXAttribute, AXUIElement};
    use accessibility_sys::{
        AXValueGetType, AXValueGetValue, AXValueRef, kAXFocusedUIElementAttribute,
        kAXPositionAttribute, kAXSizeAttribute, kAXValueTypeCGPoint, kAXValueTypeCGSize,
    };
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::string::CFString;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Pair {
        x: f64,
        y: f64,
    }

    fn unwrap_pair(value: &CFType, kind: u32) -> Option<Pair> {
        let raw = value.as_CFTypeRef() as AXValueRef;
        unsafe {
            if AXValueGetType(raw) != kind {
                return None;
            }
            let mut out = Pair::default();
            AXValueGetValue(raw, kind, (&mut out) as *mut _ as *mut std::ffi::c_void).then_some(out)
        }
    }

    fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFType> {
        element
            .attribute(&AXAttribute::new(&CFString::from_static_string(name)))
            .ok()
    }

    // 활성 앱을 pid로 직접 잡는다. 시스템 전역 요소(`AXUIElementCreateSystemWide`)를
    // 거치는 길은 최근 macOS에서 kAXErrorCannotComplete로 막혀 있다.
    let app = AXUIElement::application(pid);
    let focused = attribute(&app, kAXFocusedUIElementAttribute)?;
    let element = unsafe { AXUIElement::wrap_under_get_rule(focused.as_CFTypeRef() as _) };

    let position = unwrap_pair(
        &attribute(&element, kAXPositionAttribute)?,
        kAXValueTypeCGPoint,
    )?;
    let size = unwrap_pair(&attribute(&element, kAXSizeAttribute)?, kAXValueTypeCGSize)?;

    Some(Rect {
        x: position.x,
        y: position.y,
        width: size.x,
        height: size.y,
    })
}

/// macOS 밖에서는 아직 읽지 못한다.
/// Windows는 UI Automation으로 같은 값을 얻을 수 있다(미구현).
#[cfg(not(target_os = "macos"))]
pub fn focused_element_rect(_pid: i32) -> Option<Rect> {
    None
}

/// 컨트롤러 창이 차지한 화면 영역을 논리 px로.
pub fn window_rect(window: &tauri::WebviewWindow) -> Option<Rect> {
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().ok()?;

    Some(Rect {
        x: position.x as f64 / scale,
        y: position.y as f64 / scale,
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARGIN: f64 = 12.0;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// 컨트롤러가 우하단에 있다고 두고, 요소 위치만 바꿔 가며 본다.
    fn controller() -> Rect {
        rect(1100.0, 700.0, 360.0, 240.0)
    }

    #[test]
    fn 멀리_떨어진_요소는_가려지지_않는다() {
        assert!(!controller().overlaps(&rect(100.0, 100.0, 200.0, 40.0), MARGIN));
    }

    #[test]
    fn 창_안에_들어온_요소는_가려진다() {
        assert!(controller().overlaps(&rect(1200.0, 800.0, 120.0, 30.0), MARGIN));
    }

    #[test]
    fn 일부만_걸쳐도_가려진_것으로_본다() {
        // 왼쪽에서 창 경계를 넘어 들어온다.
        assert!(controller().overlaps(&rect(1000.0, 800.0, 200.0, 30.0), MARGIN));
    }

    #[test]
    fn 여유만큼_떨어져_있으면_가려지지_않는다() {
        // 창 왼쪽 경계에서 정확히 여유(12) 만큼 떨어진 자리.
        let element = rect(1100.0 - 40.0 - MARGIN - 1.0, 800.0, 40.0, 30.0);
        assert!(!controller().overlaps(&element, MARGIN));
    }

    #[test]
    fn 여유_안쪽으로_들어오면_가려진_것으로_본다() {
        // 붙어 있기만 해도 글자는 이미 읽기 어렵다.
        let element = rect(1100.0 - 40.0 - 2.0, 800.0, 40.0, 30.0);
        assert!(controller().overlaps(&element, MARGIN));
    }

    #[test]
    fn 크기가_없는_요소는_무시한다() {
        // 접근성 API는 숨겨진 요소에도 0 크기를 돌려줄 때가 있다.
        // 이걸 겹침으로 보면 창이 이유 없이 흐려진다.
        assert!(!controller().overlaps(&rect(1200.0, 800.0, 0.0, 0.0), MARGIN));
    }

    #[test]
    fn 가로만_겹치고_세로가_어긋나면_가려지지_않는다() {
        assert!(!controller().overlaps(&rect(1200.0, 100.0, 120.0, 30.0), MARGIN));
    }
}
