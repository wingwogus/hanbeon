//! 코어를 안드로이드에서 부를 수 있게 하는 다리.
//!
//! 여기에는 판단이 없다. 스캔 순서도 눌림 판정도 간격 조정도 전부 코어의 것이고,
//! 이 파일은 그것을 JNI 너머로 옮기기만 한다. 로직이 여기 생기면 데스크톱과
//! 안드로이드가 갈라지기 시작한다.
//!
//! 코어가 플랫폼에 요구하는 것(`Host`)은 Kotlin 쪽 객체가 답한다. 코어의 타이머
//! 스레드에서 부르게 되므로 매번 JVM에 붙였다 떨어진다.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use hanbeon_core::action::Action;
use hanbeon_core::cue::Cue;
use hanbeon_core::gesture::{GestureDetector, SharedDetector};
use hanbeon_core::host::{Host, HostError, Notice};
use hanbeon_core::journal::Journal;
use hanbeon_core::profile::{Profile, UndoMapping};
use hanbeon_core::scan::Scanner;
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint};
use jni::{JNIEnv, JavaVM};

/// 살아 있는 스캐너. 안드로이드는 앱 하나에 컨트롤러 하나다.
static RUNNING: OnceLock<Mutex<Option<Running>>> = OnceLock::new();

struct Running {
    scanner: Scanner,
    detector: SharedDetector,
}

fn running() -> &'static Mutex<Option<Running>> {
    RUNNING.get_or_init(|| Mutex::new(None))
}

/// 코어가 플랫폼에 요구하는 것을 Kotlin에게 넘긴다.
struct AndroidHost {
    vm: JavaVM,
    callbacks: GlobalRef,
}

impl AndroidHost {
    /// Kotlin 쪽 메서드를 부른다.
    ///
    /// 코어의 타이머 스레드에서 불리므로 그 스레드를 JVM에 붙여야 한다. 붙이지
    /// 않고 부르면 그 자리에서 죽는다.
    ///
    /// 돌려주는 값은 참/거짓뿐이다. 객체를 넘기면 수명이 얽히고, 어차피 코어가
    /// 알아야 하는 것은 '됐는가'뿐이다.
    fn call(&self, name: &str, sig: &str, args: &[JValue]) -> Option<bool> {
        let mut env = self.vm.attach_current_thread().ok()?;
        let result = env.call_method(&self.callbacks, name, sig, args);

        // 예외를 지우지 않으면 그 뒤의 모든 JNI 호출이 실패한다.
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
            return None;
        }

        match result {
            Ok(value) => Some(value.z().unwrap_or(true)),
            Err(_) => None,
        }
    }

    fn call_bool(&self, name: &str, arg: jint) -> bool {
        self.call(name, "(I)Z", &[JValue::Int(arg)]) == Some(true)
    }

    fn call_json(&self, name: &str, json: &str) {
        let Ok(mut env) = self.vm.attach_current_thread() else {
            return;
        };
        let Ok(text) = env.new_string(json) else {
            return;
        };
        let _ = env.call_method(
            &self.callbacks,
            name,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&JObject::from(text))],
        );
        let _ = env.exception_clear();
    }
}

/// 동작을 Kotlin이 아는 번호로. 문자열로 넘기면 오타가 런타임까지 살아남는다.
fn action_code(action: &Action) -> jint {
    match action {
        Action::Next => 0,
        Action::Prev => 1,
        Action::Enter => 2,
        Action::Settings => 3,
        Action::Shortcut(_) => 4,
    }
}

fn cue_code(cue: Cue) -> jint {
    match cue {
        Cue::Tick => 0,
        Cue::Select => 1,
        Cue::Undo => 2,
        Cue::Pause => 3,
    }
}

impl Host for AndroidHost {
    fn inject(&self, action: Action) -> Result<(), HostError> {
        // 앱별 칸(단축키)은 아직 안드로이드에 없다. 조용히 성공했다고 하면
        // 사용자는 눌렀는데 아무 일도 없는 것을 겪고 이유를 알 수 없다.
        if let Action::Shortcut(_) = action {
            return Err(HostError {
                message: "이 동작은 안드로이드에서 아직 되지 않습니다.".into(),
                needs_permission: false,
            });
        }

        if self.call_bool("inject", action_code(&action)) {
            Ok(())
        } else {
            Err(HostError {
                message: "화면을 조작하지 못했습니다. 접근성 서비스가 켜져 있는지 확인해 주세요."
                    .into(),
                needs_permission: true,
            })
        }
    }

    fn undo(&self, mapping: UndoMapping) -> Result<(), HostError> {
        let code = match mapping {
            UndoMapping::Back => 0,
            UndoMapping::Undo => 1,
        };
        if self.call_bool("undo", code) {
            Ok(())
        } else {
            Err(HostError {
                message: "되돌리지 못했습니다.".into(),
                needs_permission: false,
            })
        }
    }

    fn open_settings(&self) -> Result<(), HostError> {
        if self.call_bool("openSettings", 0) {
            Ok(())
        } else {
            Err(HostError {
                message: "설정 화면을 열지 못했습니다.".into(),
                needs_permission: false,
            })
        }
    }

    fn fit_cells(&self, extras: usize) {
        self.call("fitCells", "(I)V", &[JValue::Int(extras as jint)]);
    }

    fn cue(&self, cue: Cue) {
        self.call("cue", "(I)V", &[JValue::Int(cue_code(cue))]);
    }

    fn set_sound(&self, enabled: bool) {
        self.call("setSound", "(Z)V", &[JValue::Bool(u8::from(enabled))]);
    }

    fn publish(&self, notice: Notice) {
        match notice {
            Notice::State(snapshot) => {
                if let Ok(json) = serde_json::to_string(&*snapshot) {
                    self.call_json("publishState", &json);
                }
            }
            Notice::Error {
                message,
                needs_permission,
            } => {
                let json = serde_json::json!({
                    "message": message,
                    "needsPermission": needs_permission,
                });
                self.call_json("publishError", &json.to_string());
            }
            Notice::Interval {
                from_ms,
                to_ms,
                reason,
            } => {
                let json = serde_json::json!({
                    "fromMs": from_ms,
                    "toMs": to_ms,
                    "reason": reason,
                });
                self.call_json("publishInterval", &json.to_string());
            }
            Notice::Preset { message } => {
                let json = serde_json::json!({ "message": message });
                self.call_json("publishPreset", &json.to_string());
            }
        }
    }

    fn save_profile(&self, profile: &Profile) {
        if let Ok(json) = serde_json::to_string(profile) {
            self.call_json("saveProfile", &json);
        }
    }
}

// SAFETY: `JavaVM`과 `GlobalRef`는 스레드를 넘겨도 되는 것들이다. 코어의 타이머
// 스레드에서 부르므로 이 표시가 필요하다.
unsafe impl Send for AndroidHost {}
unsafe impl Sync for AndroidHost {}

fn read_string(env: &mut JNIEnv, value: &JString) -> String {
    env.get_string(value)
        .map(|text| text.into())
        .unwrap_or_default()
}

/// 스캐너를 띄운다. 이미 떠 있으면 아무것도 하지 않는다.
///
/// # Safety
/// JNI가 부른다. 인자는 JVM이 넘겨준 유효한 참조여야 한다.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_kr_devfive_hanbeon_Core_nativeStart(
    mut env: JNIEnv,
    _class: JClass,
    callbacks: JObject,
    profile_json: JString,
    log_dir: JString,
) -> jboolean {
    let Ok(vm) = env.get_java_vm() else {
        return 0;
    };
    let Ok(callbacks) = env.new_global_ref(callbacks) else {
        return 0;
    };

    let raw = read_string(&mut env, &profile_json);
    // 프로필을 못 읽어도 뜬다. 설정을 읽지 못했다고 앱이 안 뜨면 사용자는
    // 스위치 외의 입력 수단이 없어 아무것도 못 한다.
    let mut profile: Profile = serde_json::from_str(&raw).unwrap_or_default();
    profile.sanitize();

    let dir = read_string(&mut env, &log_dir);
    let journal = if profile.logging && !dir.is_empty() {
        Journal::open(std::path::Path::new(&dir))
    } else {
        Journal::off()
    };

    let host = Arc::new(AndroidHost { vm, callbacks });
    let long_press = std::time::Duration::from_millis(profile.long_press_ms);
    let detector: SharedDetector = Arc::new(Mutex::new(GestureDetector::new(long_press)));
    let shared = Arc::new(Mutex::new(profile));

    let scanner = Scanner::new(Arc::clone(&shared), host, journal);
    scanner.start();

    let Ok(mut slot) = running().lock() else {
        return 0;
    };
    *slot = Some(Running { scanner, detector });
    1
}

/// 스위치가 눌렸다.
///
/// # Safety
/// JNI가 부른다.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_kr_devfive_hanbeon_Core_nativePressed(_env: JNIEnv, _class: JClass) {
    let Ok(slot) = running().lock() else {
        return;
    };
    let Some(state) = slot.as_ref() else {
        return;
    };
    if let Ok(mut detector) = state.detector.lock() {
        detector.on_press(Instant::now());
    }
}

/// 스위치를 뗐다. 판정해서 코어에 넣는다.
///
/// # Safety
/// JNI가 부른다.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_kr_devfive_hanbeon_Core_nativeReleased(_env: JNIEnv, _class: JClass) {
    let Ok(slot) = running().lock() else {
        return;
    };
    let Some(state) = slot.as_ref() else {
        return;
    };

    let judged = state
        .detector
        .lock()
        .ok()
        .and_then(|mut detector| detector.on_release(Instant::now()));

    if let Some(judgement) = judged {
        state.scanner.handle(judgement);
    }
}

/// 앞에 있는 앱이 바뀌었다. 앱별 칸을 갈아 낀다.
///
/// # Safety
/// JNI가 부른다.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_kr_devfive_hanbeon_Core_nativeForeground(
    mut env: JNIEnv,
    _class: JClass,
    package: JString,
) {
    let name = read_string(&mut env, &package);
    let Ok(slot) = running().lock() else {
        return;
    };
    if let Some(state) = slot.as_ref() {
        state
            .scanner
            .apply_preset(if name.is_empty() { None } else { Some(&name) });
    }
}

/// 스위치가 빠졌다. 스캔을 정지로 내린다(PRD F10).
///
/// # Safety
/// JNI가 부른다.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_kr_devfive_hanbeon_Core_nativeSwitchLost(
    _env: JNIEnv,
    _class: JClass,
) {
    let Ok(slot) = running().lock() else {
        return;
    };
    if let Some(state) = slot.as_ref() {
        state.scanner.switch_lost();
    }
}
