//! 플랫폼에 매이지 않은 키 표현.
//!
//! 프리셋이 적는 키(`shortcut`)와 실제로 그 키를 보내는 쪽(`emit`)을 갈라 놓는다.
//! 갈라 놓는 이유는 보내는 방법이 플랫폼마다 완전히 다르기 때문이다. 데스크톱은
//! OS에 키 이벤트를 주입하지만, 안드로이드는 그게 아예 막혀 있어서 접근성
//! 서비스로 노드를 옮기거나 시스템 동작을 호출해야 한다.
//!
//! 그래서 여기에는 '무엇을 하려는가'만 적고 '어떻게 보내는가'는 적지 않는다.
//! enigo 타입을 이 안에 들이면 코어가 데스크톱 전용이 되어 버린다.

use serde::Serialize;

/// 앱별 칸이 보낼 수 있는 키.
///
/// 목록을 늘릴 때는 그 키가 어떤 플랫폼에서 무엇으로 대응되는지 확인하고 늘린다.
/// 한쪽에서만 되는 키를 넣으면 다른 플랫폼에서 칸이 조용히 사라진다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Key {
    PageDown,
    PageUp,
    Home,
    End,
    Up,
    Down,
    Left,
    Right,
    Space,
    Enter,
    Tab,
    Escape,
    /// 미디어 키는 앱이 아니라 시스템이 처리한다. 그래서 어떤 앱이 앞에 있든
    /// 같은 키로 동작하고, 안드로이드에도 같은 개념이 그대로 있다.
    MediaPlayPause,
    MediaNextTrack,
    MediaPrevTrack,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    /// 글자 하나(`cmd+o`의 `o`).
    Char(char),
}

/// 함께 누르는 보조키.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Modifier {
    /// macOS의 명령 키.
    Meta,
    Control,
    Alt,
    Shift,
}
