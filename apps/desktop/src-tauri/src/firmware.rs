//! Local-fixture firmware installer for the official Arduino Uno R3.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::arduino::{
    ArduinoCoordinator, BAUD_RATE, HANDSHAKE_REQUEST, HANDSHAKE_RESPONSE, InstallerExit,
};

pub const EVENT_FIRMWARE: &str = "arduino://firmware";
const UNO_VID: u16 = 0x2341;
const UNO_PID: u16 = 0x0043;
const UNO_FQBN: &str = "arduino:avr:uno";
const SERIAL_TIMEOUT: Duration = Duration::from_millis(500);
const REDISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const REDISCOVERY_INTERVAL: Duration = Duration::from_millis(250);
const FIXTURE_SKETCH: &str =
    "/Users/wingwogus/orca/projects/bogun/hanbeon_arduino_switch/firmware/hanbeon_arduino_switch";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArduinoCandidate {
    device_id: String,
    display_name: String,
    port: String,
    vid: u16,
    pid: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum FirmwareState {
    Searching,
    BoardFound {
        candidates: Vec<ArduinoCandidate>,
    },
    Probing {
        device_id: String,
    },
    AlreadyInstalled {
        device_id: String,
    },
    ConfirmationRequired {
        device_id: String,
        reason: ConfirmationReason,
        confirmation_token: String,
        display_name: String,
    },
    Preparing {
        device_id: String,
    },
    Uploading {
        device_id: String,
    },
    Verifying {
        device_id: String,
    },
    Complete {
        device_id: String,
    },
    Cancelled,
    Error {
        code: &'static str,
        retryable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfirmationReason {
    NoResponse,
    DifferentFirmware,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HandshakeClassification {
    Installed,
    NoResponse,
    DifferentFirmware,
}

#[derive(Clone, Debug)]
struct Confirmation {
    device_id: String,
    token: String,
}

#[derive(Default)]
struct InstallState {
    confirmation: Option<Confirmation>,
    active: bool,
}

#[derive(Clone, Default)]
pub struct FirmwareInstaller {
    inner: Arc<Mutex<InstallState>>,
    cancelled: Arc<AtomicBool>,
}

impl FirmwareInstaller {
    fn remember_confirmation(&self, device_id: &str) -> String {
        static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
        let token = format!(
            "firmware-{process}-{}",
            NEXT_TOKEN.fetch_add(1, Ordering::Relaxed),
            process = std::process::id()
        );
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .confirmation = Some(Confirmation {
            device_id: device_id.to_owned(),
            token: token.clone(),
        });
        token
    }

    fn begin(&self, device_id: &str, token: &str) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active {
            return Err("펌웨어 설치가 이미 진행 중입니다.".to_owned());
        }
        let valid = state
            .confirmation
            .as_ref()
            .is_some_and(|saved| saved.device_id == device_id && saved.token == token);
        if !valid {
            return Err("펌웨어 설치 확인 정보가 유효하지 않습니다.".to_owned());
        }
        state.confirmation = None;
        state.active = true;
        self.cancelled.store(false, Ordering::Release);
        Ok(())
    }

    fn finish(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active = false;
    }

    fn cancel(&self) -> bool {
        self.cancelled.store(true, Ordering::Release);
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.confirmation = None;
        state.active
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LocalFirmwareSource;

impl LocalFirmwareSource {
    fn sketch_path(self) -> PathBuf {
        PathBuf::from(FIXTURE_SKETCH)
    }
}

fn supported_candidate(port: serialport::SerialPortInfo) -> Option<ArduinoCandidate> {
    let serialport::SerialPortType::UsbPort(usb) = port.port_type else {
        return None;
    };
    if usb.vid != UNO_VID || usb.pid != UNO_PID {
        return None;
    }

    let display_name = usb.product.unwrap_or_else(|| "Arduino Uno R3".to_owned());
    let device_id = format!("usb-{UNO_VID:04x}-{UNO_PID:04x}-{}", port.port_name);
    Some(ArduinoCandidate {
        device_id,
        display_name,
        port: port.port_name,
        vid: usb.vid,
        pid: usb.pid,
    })
}

fn candidates() -> Result<Vec<ArduinoCandidate>, String> {
    serialport::available_ports()
        .map_err(|error| error.to_string())
        .map(|ports| ports.into_iter().filter_map(supported_candidate).collect())
}

fn find_candidate(device_id: &str) -> Result<ArduinoCandidate, String> {
    candidates()?
        .into_iter()
        .find(|candidate| candidate.device_id == device_id)
        .ok_or_else(|| "선택한 Arduino Uno를 찾지 못했습니다.".to_owned())
}

fn classify_response(response: &[u8]) -> HandshakeClassification {
    if response == HANDSHAKE_RESPONSE {
        HandshakeClassification::Installed
    } else if response.is_empty() {
        HandshakeClassification::NoResponse
    } else {
        HandshakeClassification::DifferentFirmware
    }
}

fn probe_port(port: &str) -> Result<HandshakeClassification, String> {
    let mut serial = serialport::new(port, BAUD_RATE)
        .timeout(SERIAL_TIMEOUT)
        .open()
        .map_err(|error| error.to_string())?;

    let mut received = Vec::with_capacity(HANDSHAKE_RESPONSE.len());
    for _ in 0..4 {
        serial
            .write_all(HANDSHAKE_REQUEST)
            .and_then(|()| serial.flush())
            .map_err(|error| error.to_string())?;
        let mut byte = [0_u8; 1];
        loop {
            match serial.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    received.push(byte[0]);
                    if byte[0] == b'\n' || received.len() >= HANDSHAKE_RESPONSE.len() {
                        return Ok(classify_response(&received));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        if !received.is_empty() {
            return Ok(classify_response(&received));
        }
    }
    Ok(HandshakeClassification::NoResponse)
}

fn emit(app: &AppHandle, state: &FirmwareState) {
    if let Err(error) = app.emit(EVENT_FIRMWARE, state) {
        eprintln!("Arduino firmware event를 보내지 못했습니다. {error}");
    }
}

#[tauri::command]
pub fn list_arduino_candidates(app: AppHandle) -> Result<Vec<ArduinoCandidate>, String> {
    emit(&app, &FirmwareState::Searching);
    let found = candidates()?;
    if found.is_empty() {
        emit(
            &app,
            &FirmwareState::Error {
                code: "notFound",
                retryable: true,
            },
        );
    } else {
        emit(
            &app,
            &FirmwareState::BoardFound {
                candidates: found.clone(),
            },
        );
    }
    Ok(found)
}

#[tauri::command]
pub fn probe_arduino_firmware(
    app: AppHandle,
    device_id: String,
    installer: State<'_, FirmwareInstaller>,
    coordinator: State<'_, ArduinoCoordinator>,
) -> Result<FirmwareState, String> {
    let candidate = find_candidate(&device_id)?;
    emit(
        &app,
        &FirmwareState::Probing {
            device_id: device_id.clone(),
        },
    );

    let ownership = coordinator.acquire_installer().map_err(|_| {
        emit(
            &app,
            &FirmwareState::Error {
                code: "portUnavailable",
                retryable: true,
            },
        );
        "Arduino 포트를 사용할 수 없습니다.".to_owned()
    })?;
    let classification = probe_port(&candidate.port);
    ownership.finish(if classification.is_ok() {
        InstallerExit::Success
    } else {
        InstallerExit::Failure
    });
    let classification = classification.inspect_err(|_| {
        emit(
            &app,
            &FirmwareState::Error {
                code: "portUnavailable",
                retryable: true,
            },
        );
    })?;

    let state = match classification {
        HandshakeClassification::Installed => FirmwareState::AlreadyInstalled { device_id },
        HandshakeClassification::NoResponse | HandshakeClassification::DifferentFirmware => {
            let reason = if classification == HandshakeClassification::NoResponse {
                ConfirmationReason::NoResponse
            } else {
                ConfirmationReason::DifferentFirmware
            };
            FirmwareState::ConfirmationRequired {
                confirmation_token: installer.remember_confirmation(&device_id),
                device_id,
                reason,
                display_name: candidate.display_name,
            }
        }
    };
    emit(&app, &state);
    Ok(state)
}

fn compile_args(sketch: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "compile".into(),
        "--fqbn".into(),
        UNO_FQBN.into(),
        "--output-dir".into(),
        output.as_os_str().to_owned(),
        sketch.as_os_str().to_owned(),
    ]
}

fn upload_args(port: &str, output: &Path) -> Vec<OsString> {
    vec![
        "upload".into(),
        "--fqbn".into(),
        UNO_FQBN.into(),
        "--port".into(),
        port.into(),
        "--input-dir".into(),
        output.as_os_str().to_owned(),
    ]
}

fn bundled_cli() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let name = if cfg!(windows) {
        "arduino-cli.exe"
    } else {
        "arduino-cli"
    };
    Ok(executable
        .parent()
        .ok_or_else(|| "실행 파일 위치를 찾지 못했습니다.".to_owned())?
        .join(name))
}

fn run_cli(cli: &Path, args: &[OsString]) -> Result<(), String> {
    let output = Command::new(cli)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

fn rediscover(previous_port: &str, cancelled: &AtomicBool) -> Option<ArduinoCandidate> {
    let deadline = Instant::now() + REDISCOVERY_TIMEOUT;
    while Instant::now() < deadline && !cancelled.load(Ordering::Acquire) {
        if let Ok(found) = candidates()
            && let Some(candidate) = found
                .iter()
                .find(|candidate| candidate.port == previous_port)
                .cloned()
                .or_else(|| (found.len() == 1).then(|| found[0].clone()))
        {
            return Some(candidate);
        }
        thread::sleep(REDISCOVERY_INTERVAL);
    }
    None
}

#[derive(Clone, Copy)]
enum InstallFailure {
    Cancelled,
    Upload,
    Verify,
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), InstallFailure> {
    (!cancelled.load(Ordering::Acquire))
        .then_some(())
        .ok_or(InstallFailure::Cancelled)
}

fn install(
    app: &AppHandle,
    device_id: &str,
    candidate: &ArduinoCandidate,
    cancelled: &AtomicBool,
) -> Result<(), InstallFailure> {
    let source = LocalFirmwareSource.sketch_path();
    if !source.join("hanbeon_arduino_switch.ino").is_file() {
        return Err(InstallFailure::Upload);
    }
    let output = std::env::temp_dir().join(format!("hanbeon-firmware-{}", std::process::id()));
    let _ = fs::remove_dir_all(&output);
    fs::create_dir_all(&output).map_err(|_| InstallFailure::Upload)?;
    let cli = bundled_cli().map_err(|_| InstallFailure::Upload)?;

    ensure_not_cancelled(cancelled)?;
    run_cli(&cli, &compile_args(&source, &output)).map_err(|_| InstallFailure::Upload)?;
    ensure_not_cancelled(cancelled)?;
    emit(
        app,
        &FirmwareState::Uploading {
            device_id: device_id.to_owned(),
        },
    );
    run_cli(&cli, &upload_args(&candidate.port, &output)).map_err(|_| InstallFailure::Upload)?;
    ensure_not_cancelled(cancelled)?;
    emit(
        app,
        &FirmwareState::Verifying {
            device_id: device_id.to_owned(),
        },
    );
    let rediscovered = rediscover(&candidate.port, cancelled).ok_or(InstallFailure::Verify)?;
    ensure_not_cancelled(cancelled)?;
    let verified = probe_port(&rediscovered.port).map_err(|_| InstallFailure::Verify)?;
    let _ = fs::remove_dir_all(output);
    if verified == HandshakeClassification::Installed {
        Ok(())
    } else {
        Err(InstallFailure::Verify)
    }
}

#[tauri::command]
pub fn begin_firmware_install(
    app: AppHandle,
    device_id: String,
    confirmation_token: String,
    installer: State<'_, FirmwareInstaller>,
    coordinator: State<'_, ArduinoCoordinator>,
) -> Result<(), String> {
    installer.begin(&device_id, &confirmation_token)?;
    let installer = installer.inner().clone();
    let coordinator = coordinator.inner().clone();
    thread::spawn(move || {
        emit(
            &app,
            &FirmwareState::Preparing {
                device_id: device_id.clone(),
            },
        );
        let candidate = find_candidate(&device_id);
        let ownership = coordinator.acquire_installer();
        let result = match (candidate, ownership) {
            (Ok(candidate), Ok(ownership)) => {
                let result = install(&app, &device_id, &candidate, &installer.cancelled);
                let exit = match result {
                    Ok(()) => InstallerExit::Success,
                    Err(InstallFailure::Cancelled) => InstallerExit::Cancelled,
                    Err(_) => InstallerExit::Failure,
                };
                ownership.finish(exit);
                result
            }
            _ => Err(InstallFailure::Upload),
        };

        let state = match result {
            Ok(()) => FirmwareState::Complete { device_id },
            Err(InstallFailure::Cancelled) => FirmwareState::Cancelled,
            Err(InstallFailure::Upload) => FirmwareState::Error {
                code: "uploadFailed",
                retryable: true,
            },
            Err(InstallFailure::Verify) => FirmwareState::Error {
                code: "verifyFailed",
                retryable: true,
            },
        };
        installer.finish();
        emit(&app, &state);
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_firmware_install(
    app: AppHandle,
    installer: State<'_, FirmwareInstaller>,
) -> Result<(), String> {
    if !installer.cancel() {
        emit(&app, &FirmwareState::Cancelled);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arduino::{ArduinoOwner, test_support};
    use serialport::{SerialPortInfo, SerialPortType, UsbPortInfo};

    fn usb_port(vid: u16, pid: u16) -> SerialPortInfo {
        SerialPortInfo {
            port_name: "/dev/cu.usbmodem1".to_owned(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: None,
                manufacturer: Some("Arduino".to_owned()),
                product: Some("Uno".to_owned()),
            }),
        }
    }

    #[test]
    fn supported_device_filter_accepts_only_official_uno_r3() {
        assert!(supported_candidate(usb_port(UNO_VID, UNO_PID)).is_some());
        assert!(supported_candidate(usb_port(0x2a03, UNO_PID)).is_none());
        assert!(
            supported_candidate(SerialPortInfo {
                port_name: "ttyS0".to_owned(),
                port_type: SerialPortType::Unknown,
            })
            .is_none()
        );
    }

    #[test]
    fn cli_arguments_are_structured_for_uno_compile_and_upload() {
        let compile = compile_args(Path::new("fixture"), Path::new("build"));
        let upload = upload_args("COM7", Path::new("build"));
        let strings = |args: &[OsString]| {
            args.iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            strings(&compile),
            [
                "compile",
                "--fqbn",
                UNO_FQBN,
                "--output-dir",
                "build",
                "fixture"
            ]
        );
        assert_eq!(
            strings(&upload),
            [
                "upload",
                "--fqbn",
                UNO_FQBN,
                "--port",
                "COM7",
                "--input-dir",
                "build"
            ]
        );
    }

    #[test]
    fn handshake_classification_and_installer_seam_restore_connection() {
        assert_eq!(
            classify_response(HANDSHAKE_RESPONSE),
            HandshakeClassification::Installed
        );
        assert_eq!(classify_response(b""), HandshakeClassification::NoResponse);
        assert_eq!(
            classify_response(b"OTHER\n"),
            HandshakeClassification::DifferentFirmware
        );

        let coordinator = ArduinoCoordinator::new(test_support::coordinator_probe_silent);
        {
            let _ownership = coordinator.acquire_installer().unwrap();
            assert_eq!(coordinator.owner(), ArduinoOwner::Installer);
        }
        assert_eq!(coordinator.owner(), ArduinoOwner::Connection);
    }
}
