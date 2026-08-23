//! 대상 앱으로의 키 주입.
//!
//! 대상은 '현재 포커스를 가진 앱'이다. 우리가 대상을 고르지 않는다.
//! 그래서 이 모듈이 성립하려면 floating 창이 포커스를 뺏지 않아야 한다(`window` 참조).

use enigo::{Direction, Enigo, InputError, Key, Keyboard, Settings};

use crate::action::Action;
use crate::profile::UndoMapping;
use crate::shortcut::Shortcut;

/// 주입 실패 원인. 사용자에게 그대로 보여줄 수 있는 문구를 담는다.
#[derive(Debug)]
pub struct EmitError {
    pub message: String,
    /// 권한 문제로 보이는 경우. 화면에서 해결 방법을 함께 안내한다.
    pub needs_permission: bool,
}

impl EmitError {
    /// 키 주입이 아닌 경로(설정 창 열기 등)의 실패를 같은 통로로 흘린다.
    pub fn from_message(message: String) -> Self {
        Self {
            message,
            needs_permission: false,
        }
    }
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// `Enigo`를 상태로 들고 있지 않고 매번 만든다. macOS의 이벤트 소스는 스레드
/// 경계를 넘기기 까다로운 반면, 생성 비용은 주입 예산(100ms)에 비해 무시할 수준이다.
fn keyboard() -> Result<Enigo, EmitError> {
    Enigo::new(&Settings::default()).map_err(|e| EmitError {
        message: format!("키를 보낼 수 없습니다. 손쉬운 사용(접근성) 권한을 확인해 주세요. ({e})"),
        needs_permission: true,
    })
}

fn injected(result: Result<(), InputError>) -> Result<(), EmitError> {
    result.map_err(|e| EmitError {
        message: format!("키 입력을 보내지 못했습니다. ({e})"),
        needs_permission: false,
    })
}

/// 보조키를 누른 채 키 하나를 치고, 반드시 보조키를 놓는다.
///
/// 중간에 실패해도 놓는 것은 건너뛰지 않는다. 보조키가 눌린 채로 남으면
/// 그 뒤의 모든 입력이 조합키가 되어, 사용자는 무엇을 눌러도 엉뚱한 일이
/// 일어나는 상태에 갇힌다.
fn send_shortcut(enigo: &mut Enigo, shortcut: &Shortcut) -> Result<(), InputError> {
    let mut result = Ok(());

    for modifier in &shortcut.modifiers {
        result = result.and_then(|_| enigo.key(modifier.key(), Direction::Press));
    }
    result = result.and_then(|_| enigo.key(shortcut.key, Direction::Click));

    for modifier in shortcut.modifiers.iter().rev() {
        let released = enigo.key(modifier.key(), Direction::Release);
        if result.is_ok() {
            result = released;
        }
    }

    result
}

/// 대상 앱에 동작을 주입한다.
pub fn send(action: Action) -> Result<(), EmitError> {
    let mut enigo = keyboard()?;

    injected(match action {
        Action::Next => enigo.key(Key::Tab, Direction::Click),
        Action::Prev => enigo
            .key(Key::Shift, Direction::Press)
            .and_then(|_| enigo.key(Key::Tab, Direction::Click))
            .and_then(|_| enigo.key(Key::Shift, Direction::Release)),
        Action::Enter => enigo.key(Key::Return, Direction::Click),
        // 설정 창 열기는 키 주입이 아니라 우리 앱 내부 동작이다.
        Action::Settings => Ok(()),
        Action::Shortcut(shortcut) => send_shortcut(&mut enigo, &shortcut),
    })
}

/// 되돌리기.
///
/// 대상 앱이 이 단축키를 지원할 때만 실제로 되돌아간다. 모든 앱에서 되돌릴 수
/// 있다고 사용자에게 약속하지 않는다(PRD F6).
pub fn send_undo(mapping: UndoMapping) -> Result<(), EmitError> {
    let mut enigo = keyboard()?;

    // macOS의 명령 키는 Meta, 그 외 플랫폼의 실행 취소는 Ctrl 조합이다.
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    let result = match mapping {
        UndoMapping::Back => {
            #[cfg(target_os = "macos")]
            {
                enigo
                    .key(Key::Meta, Direction::Press)
                    .and_then(|_| enigo.key(Key::Unicode('['), Direction::Click))
                    .and_then(|_| enigo.key(Key::Meta, Direction::Release))
            }
            #[cfg(not(target_os = "macos"))]
            {
                enigo
                    .key(Key::Alt, Direction::Press)
                    .and_then(|_| enigo.key(Key::LeftArrow, Direction::Click))
                    .and_then(|_| enigo.key(Key::Alt, Direction::Release))
            }
        }
        UndoMapping::Undo => enigo
            .key(modifier, Direction::Press)
            .and_then(|_| enigo.key(Key::Unicode('z'), Direction::Click))
            .and_then(|_| enigo.key(modifier, Direction::Release)),
    };

    injected(result)
}
