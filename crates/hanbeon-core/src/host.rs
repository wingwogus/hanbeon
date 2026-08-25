//! 코어가 플랫폼에 요구하는 것.
//!
//! 스캔 상태기계는 '언제 무엇을 할지'를 정하고, 그것을 '어떻게 하는지'는 모른다.
//! 여기가 그 경계다.
//!
//! 경계를 두는 이유는 플랫폼마다 실행 방법이 근본적으로 다르기 때문이다.
//! 데스크톱은 OS에 키 이벤트를 주입하지만, 안드로이드는 다른 앱으로의 키 주입을
//! 아예 막아 두어서 접근성 서비스로 포커스를 옮기는 방식밖에 없다. `>`가 데스크톱에서는
//! `Tab`이고 안드로이드에서는 `focusSearch(FOCUS_FORWARD)`가 된다. 그래도 '다음 요소로
//! 옮긴다'는 결정 자체는 같으므로, 그 결정을 내리는 코드는 한 벌이면 된다.

use crate::action::Action;
use crate::cue::Cue;
use crate::profile::{Profile, UndoMapping};
use crate::scan::Snapshot;

/// 실행 실패. 사용자에게 그대로 보여줄 수 있는 문구를 담는다.
pub struct HostError {
    pub message: String,
    /// 권한 문제로 보이는 경우. 화면에서 해결 방법을 함께 안내한다.
    pub needs_permission: bool,
}

/// 화면으로 내보낼 것.
pub enum Notice {
    /// 지금 커서가 어디에 있고 무엇이 보여야 하는지.
    State(Box<Snapshot>),
    Error {
        message: String,
        needs_permission: bool,
    },
    /// 간격이 바뀐 이유. 이유를 알 수 없는 변화는 통제감을 무너뜨린다(원칙 2).
    Interval {
        from_ms: u64,
        to_ms: u64,
        reason: String,
    },
    /// 앱별 칸이 갈렸다.
    Preset { message: String },
}

pub trait Host: Send + Sync + 'static {
    /// 대상 앱에 동작을 보낸다.
    fn inject(&self, action: Action) -> Result<(), HostError>;

    /// 되돌리기. 대상 앱이 그 단축키를 지원할 때만 실제로 되돌아간다(PRD F6).
    fn undo(&self, mapping: UndoMapping) -> Result<(), HostError>;

    /// 설정 화면을 연다. 키 주입이 아니라 우리 앱 내부 동작이다.
    fn open_settings(&self) -> Result<(), HostError>;

    /// 칸 수가 달라졌으니 화면을 거기에 맞춘다.
    ///
    /// 화면과 실제 칸이 어긋나면 사용자는 보이지 않는 칸을 누르게 된다.
    fn fit_cells(&self, extras: usize);

    /// 두 감각 중 소리 쪽(원칙 4).
    fn cue(&self, cue: Cue);

    fn set_sound(&self, enabled: bool);

    fn publish(&self, notice: Notice);

    /// 조정된 간격을 파일에 적는다.
    ///
    /// 조정될 때마다 적는다. 종료 시에만 저장하면 강제 종료에서 사용자가
    /// 익숙해진 속도를 잃는다.
    fn save_profile(&self, profile: &Profile);
}

/// 아무것도 하지 않는 Host. 안드로이드의 실제 구현이 붙기 전까지 커맨드
/// 경로가 컴파일되도록 하는 자리표시자다. 판단 로직은 코어에 그대로 있고,
/// 실행만 비어 있다.
#[derive(Default)]
pub struct NoopHost;

impl Host for NoopHost {
    fn inject(&self, _action: Action) -> Result<(), HostError> {
        Err(HostError {
            message: "안드로이드 호스트가 아직 연결되지 않았습니다.".to_string(),
            needs_permission: false,
        })
    }

    fn undo(&self, _mapping: UndoMapping) -> Result<(), HostError> {
        Err(HostError {
            message: "안드로이드 호스트가 아직 연결되지 않았습니다.".to_string(),
            needs_permission: false,
        })
    }

    fn open_settings(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn fit_cells(&self, _extras: usize) {}

    fn cue(&self, _cue: Cue) {}

    fn set_sound(&self, _enabled: bool) {}

    fn publish(&self, _notice: Notice) {}

    fn save_profile(&self, _profile: &Profile) {}
}
