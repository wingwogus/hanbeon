//! 앱별 칸이 보낼 키 조합.
//!
//! 프리셋은 `"pagedown"`, `"cmd+o"`, `"mediaplaypause"` 같은 문자열로 키를 적는다.
//! 여기서 그것을 실제로 보낼 수 있는 형태로 바꾼다.
//!
//! 해석하지 못한 문자열은 조용히 넘기지 않는다. 없는 칸이 생기면 사용자는
//! 자기가 익힌 자리에서 다른 동작이 나오는 것을 겪게 되고, 그게 왜인지 알
//! 방법이 없다. 프리셋을 만들 때 걸러 내고, 걸러졌다는 사실을 남긴다.

use enigo::Key;
use serde::Serialize;

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

impl Modifier {
    pub fn key(self) -> Key {
        match self {
            Modifier::Meta => Key::Meta,
            Modifier::Control => Key::Control,
            Modifier::Alt => Key::Alt,
            Modifier::Shift => Key::Shift,
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "cmd" | "command" | "meta" | "super" => Some(Modifier::Meta),
            "ctrl" | "control" => Some(Modifier::Control),
            "alt" | "option" => Some(Modifier::Alt),
            "shift" => Some(Modifier::Shift),
            _ => None,
        }
    }
}

/// 보낼 키 하나와, 함께 누를 보조키들.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub modifiers: Vec<Modifier>,
    pub key: Key,
}

/// 보조키 없이 단독으로 쓰는 키 이름.
fn plain_key(token: &str) -> Option<Key> {
    Some(match token {
        "pagedown" | "pgdn" => Key::PageDown,
        "pageup" | "pgup" => Key::PageUp,
        "home" => Key::Home,
        "end" => Key::End,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "space" => Key::Space,
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        // 미디어 키는 앱 전용 단축키가 아니라 시스템이 처리한다. 그래서 어떤
        // 음악 앱이 앞에 있든 같은 키로 동작한다.
        "mediaplaypause" | "playpause" => Key::MediaPlayPause,
        "medianexttrack" | "nexttrack" => Key::MediaNextTrack,
        "mediaprevtrack" | "prevtrack" => Key::MediaPrevTrack,
        "volumeup" => Key::VolumeUp,
        "volumedown" => Key::VolumeDown,
        "volumemute" | "mute" => Key::VolumeMute,
        _ => return None,
    })
}

/// `"cmd+o"`, `"pagedown"` 같은 표기를 해석한다. 해석하지 못하면 `None`.
pub fn parse(spec: &str) -> Option<Shortcut> {
    let spec = spec.trim().to_ascii_lowercase();
    if spec.is_empty() {
        return None;
    }

    let mut tokens: Vec<&str> = spec.split('+').map(str::trim).collect();
    let last = tokens.pop()?;

    let mut modifiers = Vec::new();
    for token in tokens {
        modifiers.push(Modifier::parse(token)?);
    }

    // 마지막 자리는 이름 있는 키이거나, 글자 하나(`cmd+o`의 `o`)다.
    let key = match plain_key(last) {
        Some(key) => key,
        None => {
            let mut chars = last.chars();
            match (chars.next(), chars.next()) {
                (Some(single), None) => Key::Unicode(single),
                _ => return None,
            }
        }
    };

    Some(Shortcut { modifiers, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 이름_있는_키를_해석한다() {
        assert_eq!(
            parse("pagedown"),
            Some(Shortcut {
                modifiers: vec![],
                key: Key::PageDown
            })
        );
    }

    #[test]
    fn 보조키_조합을_해석한다() {
        assert_eq!(
            parse("cmd+o"),
            Some(Shortcut {
                modifiers: vec![Modifier::Meta],
                key: Key::Unicode('o')
            })
        );
    }

    #[test]
    fn 보조키를_여러_개_받는다() {
        let parsed = parse("cmd+shift+z").expect("해석되어야 한다");
        assert_eq!(parsed.modifiers, vec![Modifier::Meta, Modifier::Shift]);
        assert_eq!(parsed.key, Key::Unicode('z'));
    }

    #[test]
    fn 대소문자와_공백을_가리지_않는다() {
        assert_eq!(parse("  Cmd + O "), parse("cmd+o"));
    }

    #[test]
    fn 미디어_키는_보조키_없이_쓴다() {
        assert_eq!(
            parse("mediaplaypause"),
            Some(Shortcut {
                modifiers: vec![],
                key: Key::MediaPlayPause
            })
        );
    }

    #[test]
    fn 모르는_이름은_해석하지_않는다() {
        // 조용히 아무 키나 보내면 사용자는 익힌 자리에서 다른 동작을 겪는다.
        assert_eq!(parse("페이지다운"), None);
        assert_eq!(parse("cmd+unknownkey"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn 모르는_보조키는_해석하지_않는다() {
        assert_eq!(parse("hyper+o"), None);
    }
}
