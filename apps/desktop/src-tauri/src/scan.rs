//! Desktop host for the platform-independent scan state machine.

#[cfg(feature = "desktop")]
use std::path::PathBuf;
#[cfg(feature = "desktop")]
use std::sync::Arc;

#[cfg(feature = "desktop")]
use hanbeon_core::action::Action;
#[cfg(feature = "desktop")]
use hanbeon_core::cue::Cue;
#[cfg(feature = "desktop")]
use hanbeon_core::host::{Host, HostError, Notice};
#[cfg(feature = "desktop")]
use hanbeon_core::profile::{Profile, UndoMapping};
#[cfg(feature = "desktop")]
use hanbeon_core::scan::Snapshot;
#[cfg(feature = "desktop")]
use serde::Serialize;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(feature = "desktop")]
use crate::audio::Audio;
#[cfg(feature = "desktop")]
use crate::led::LedBridge;
#[cfg(feature = "desktop")]
use crate::{emit, window};

pub use hanbeon_core::scan::interval_override;

#[cfg(feature = "desktop")]
pub const EVENT_STATE: &str = "scan://state";
#[cfg(feature = "desktop")]
pub const EVENT_ERROR: &str = "scan://error";
#[cfg(feature = "desktop")]
pub const EVENT_INTERVAL: &str = "scan://interval";
#[cfg(feature = "desktop")]
pub const EVENT_PRESET: &str = "scan://preset";

#[cfg(feature = "desktop")]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload {
    message: String,
    needs_permission: bool,
}

#[cfg(feature = "desktop")]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PresetPayload {
    message: String,
}

#[cfg(feature = "desktop")]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntervalPayload {
    from_ms: u64,
    to_ms: u64,
    reason: String,
}

#[cfg(feature = "desktop")]
pub struct DesktopHost {
    app: AppHandle,
    audio: Audio,
    led: Arc<LedBridge>,
    config_dir: PathBuf,
}

#[cfg(feature = "desktop")]
impl DesktopHost {
    pub fn new(app: AppHandle, audio: Audio, config_dir: PathBuf) -> Self {
        Self {
            app,
            audio,
            led: Arc::new(LedBridge::native_transport()),
            config_dir,
        }
    }

    pub fn sync_led(&self, snapshot: &Snapshot) {
        self.led.sync(snapshot);
    }
}

#[cfg(feature = "desktop")]
fn host_error(error: emit::EmitError) -> HostError {
    HostError {
        message: error.message,
        needs_permission: error.needs_permission,
    }
}

#[cfg(feature = "desktop")]
impl Host for DesktopHost {
    fn inject(&self, action: Action) -> Result<(), HostError> {
        emit::send(action).map_err(host_error)
    }

    fn undo(&self, mapping: UndoMapping) -> Result<(), HostError> {
        emit::send_undo(mapping).map_err(host_error)
    }

    fn open_settings(&self) -> Result<(), HostError> {
        window::show_settings(&self.app).map_err(|message| HostError {
            message,
            needs_permission: false,
        })
    }

    fn fit_cells(&self, extras: usize) {
        if let Some(window) = self.app.get_webview_window("floating") {
            let _ = window::fit_cells(&window, extras);
        }
    }

    fn cue(&self, cue: Cue) {
        self.audio.play(cue);
    }

    fn set_sound(&self, enabled: bool) {
        self.audio.set_enabled(enabled);
    }

    fn publish(&self, notice: Notice) {
        match notice {
            Notice::State(snapshot) => {
                self.led.sync(&snapshot);
                let _ = self.app.emit(EVENT_STATE, *snapshot);
            }
            Notice::Error {
                message,
                needs_permission,
            } => {
                let _ = self.app.emit(
                    EVENT_ERROR,
                    ErrorPayload {
                        message,
                        needs_permission,
                    },
                );
            }
            Notice::Interval {
                from_ms,
                to_ms,
                reason,
            } => {
                let _ = self.app.emit(
                    EVENT_INTERVAL,
                    IntervalPayload {
                        from_ms,
                        to_ms,
                        reason,
                    },
                );
            }
            Notice::Preset { message } => {
                let _ = self.app.emit(EVENT_PRESET, PresetPayload { message });
            }
        }
    }

    fn save_profile(&self, profile: &Profile) {
        if let Err(message) = profile.save(&self.config_dir) {
            eprintln!("조정된 속도를 저장하지 못했습니다. {message}");
        }
    }
}
