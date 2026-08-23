//! Local-fixture firmware installer for the official Arduino Uno R3.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
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
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupMode {
    Connect,
    Setup,
}

fn startup_mode_for(handshakes: impl IntoIterator<Item = HandshakeClassification>) -> StartupMode {
    let mut found = false;
    for handshake in handshakes {
        found = true;
        if handshake == HandshakeClassification::Installed {
            return StartupMode::Connect;
        }
    }
    if found {
        StartupMode::Setup
    } else {
        StartupMode::Connect
    }
}

pub fn startup_mode() -> StartupMode {
    let supported = candidates().unwrap_or_default();
    startup_mode_for(supported.iter().map(|candidate| {
        probe_port(&candidate.port).unwrap_or(HandshakeClassification::NoResponse)
    }))
}

#[derive(Clone, Debug)]
struct Confirmation {
    candidate: ArduinoCandidate,
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
    fn remember_confirmation(&self, candidate: ArduinoCandidate) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .confirmation = Some(Confirmation { candidate });
    }

    fn begin(&self, device_id: &str) -> Result<ArduinoCandidate, String> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.active {
            return Err("펌웨어 설치가 이미 진행 중입니다.".to_owned());
        }
        let candidate = state
            .confirmation
            .as_ref()
            .filter(|saved| saved.candidate.device_id == device_id)
            .map(|saved| saved.candidate.clone())
            .ok_or_else(|| "펌웨어 설치 확인 정보가 유효하지 않습니다.".to_owned())?;
        state.confirmation = None;
        state.active = true;
        self.cancelled.store(false, Ordering::Release);
        Ok(candidate)
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

fn preferred_serial_path(name: &str) -> bool {
    !name.contains("/tty.") && !name.contains(r"\\.\COM")
}

fn supported_candidate(port: serialport::SerialPortInfo) -> Option<ArduinoCandidate> {
    let serialport::SerialPortType::UsbPort(usb) = port.port_type else {
        return None;
    };
    if usb.vid != UNO_VID || usb.pid != UNO_PID {
        return None;
    }
    if cfg!(unix) && !port.port_name.contains("/cu.") {
        return None;
    }

    let display_name = usb.product.unwrap_or_else(|| "Arduino Uno R3".to_owned());
    let device_id = usb.serial_number.as_deref().map_or_else(
        || format!("usb-{UNO_VID:04x}-{UNO_PID:04x}-{}", port.port_name),
        |serial| format!("usb-{UNO_VID:04x}-{UNO_PID:04x}-{serial}"),
    );
    Some(ArduinoCandidate {
        device_id,
        display_name,
        port: port.port_name,
        vid: usb.vid,
        pid: usb.pid,
    })
}

fn candidates() -> Result<Vec<ArduinoCandidate>, String> {
    let mut found: Vec<ArduinoCandidate> = serialport::available_ports()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(supported_candidate)
        .collect();
    found.sort_by(|left, right| {
        preferred_serial_path(&left.port)
            .cmp(&preferred_serial_path(&right.port))
            .reverse()
            .then_with(|| left.port.cmp(&right.port))
    });
    found.dedup_by(|left, right| left.device_id == right.device_id);
    Ok(found)
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

fn open_probe_port(port: &str) -> Result<Box<dyn serialport::SerialPort>, String> {
    let mut last_error = None;
    for _ in 0..8 {
        match serialport::new(port, BAUD_RATE)
            .timeout(SERIAL_TIMEOUT)
            .exclusive(true)
            .open()
        {
            Ok(serial) => return Ok(serial),
            Err(error) => {
                let message = format!("{port}: {error}");
                let busy = message.to_ascii_lowercase().contains("busy") || message.contains("16");
                last_error = Some(message);
                if !busy {
                    break;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| format!("{port}: couldn't open")))
}

fn probe_port(port: &str) -> Result<HandshakeClassification, String> {
    let mut serial = open_probe_port(port)?;

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
                        let classified = classify_response(&received);
                        drop(serial);
                        return Ok(classified);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        if !received.is_empty() {
            let classified = classify_response(&received);
            drop(serial);
            return Ok(classified);
        }
    }
    drop(serial);
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
                detail: None,
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

    let ownership = coordinator.acquire_setup_probe().map_err(|_| {
        emit(
            &app,
            &FirmwareState::Error {
                code: "portUnavailable",
                retryable: true,
                detail: None,
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
    let classification = classification.inspect_err(|error| {
        emit(
            &app,
            &FirmwareState::Error {
                code: "portUnavailable",
                retryable: true,
                detail: Some(error.clone()),
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
            installer.remember_confirmation(candidate.clone());
            FirmwareState::ConfirmationRequired {
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

fn core_install_args() -> Vec<OsString> {
    ["core", "install", "arduino:avr"]
        .into_iter()
        .map(OsString::from)
        .collect()
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

fn cli_home() -> Result<PathBuf, String> {
    let home = std::env::temp_dir().join("hanbeon-arduino-cli");
    fs::create_dir_all(&home).map_err(|error| error.to_string())?;
    Ok(home)
}

fn cli_env(cli: &Path) -> Result<Vec<(OsString, OsString)>, String> {
    let home = cli_home()?;
    Ok(vec![
        (
            "ARDUINO_DIRECTORIES_DATA".into(),
            home.as_os_str().to_owned(),
        ),
        (
            "ARDUINO_DIRECTORIES_DOWNLOADS".into(),
            home.join("staging").as_os_str().to_owned(),
        ),
        (
            "ARDUINO_DIRECTORIES_USER".into(),
            home.join("user").as_os_str().to_owned(),
        ),
        ("HOME".into(), home.as_os_str().to_owned()),
        (
            "PATH".into(),
            format!(
                "{}{}{}",
                cli.parent().unwrap_or_else(|| Path::new(".")).display(),
                std::path::MAIN_SEPARATOR,
                std::env::var("PATH").unwrap_or_default()
            )
            .into(),
        ),
    ])
}

fn log_firmware(message: &str) {
    eprintln!("{message}");
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hanbeon-firmware.log")
    {
        let _ = writeln!(file, "{message}");
    }
}

fn run_cli(cli: &Path, args: &[OsString]) -> Result<(), String> {
    log_firmware(&format!("[firmware] cli: {} {:?}", cli.display(), args));
    let mut command = Command::new(cli);
    command.args(args);
    for (key, value) in cli_env(cli)? {
        command.env(key, value);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !stdout.is_empty() {
            log_firmware(&format!("[firmware] cli stdout: {stdout}"));
        }
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        log_firmware(&format!(
            "[firmware] cli failed ({:?}): {stderr}",
            output.status.code()
        ));
        Err(stderr)
    }
}

fn rediscover(previous: &ArduinoCandidate, cancelled: &AtomicBool) -> Option<ArduinoCandidate> {
    let deadline = Instant::now() + REDISCOVERY_TIMEOUT;
    while Instant::now() < deadline && !cancelled.load(Ordering::Acquire) {
        if let Ok(found) = candidates()
            && let Some(candidate) = found
                .iter()
                .find(|candidate| candidate.device_id == previous.device_id)
                .cloned()
                .or_else(|| {
                    found
                        .iter()
                        .find(|candidate| candidate.port == previous.port)
                        .cloned()
                })
                .or_else(|| (found.len() == 1).then(|| found[0].clone()))
        {
            return Some(candidate);
        }
        thread::sleep(REDISCOVERY_INTERVAL);
    }
    None
}

#[derive(Clone, Debug)]
enum InstallFailure {
    Cancelled,
    Upload(String),
    Verify(String),
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
        return Err(InstallFailure::Upload(format!(
            "local sketch missing: {}",
            source.display()
        )));
    }
    let output = std::env::temp_dir().join(format!("hanbeon-firmware-{}", std::process::id()));
    let _ = fs::remove_dir_all(&output);
    fs::create_dir_all(&output).map_err(|error| {
        InstallFailure::Upload(format!("output dir {}: {error}", output.display()))
    })?;
    let cli = bundled_cli().map_err(InstallFailure::Upload)?;

    ensure_not_cancelled(cancelled)?;
    run_cli(&cli, &core_install_args()).map_err(InstallFailure::Upload)?;
    ensure_not_cancelled(cancelled)?;
    run_cli(&cli, &compile_args(&source, &output)).map_err(InstallFailure::Upload)?;
    ensure_not_cancelled(cancelled)?;
    emit(
        app,
        &FirmwareState::Uploading {
            device_id: device_id.to_owned(),
        },
    );
    run_cli(&cli, &upload_args(&candidate.port, &output)).map_err(InstallFailure::Upload)?;
    ensure_not_cancelled(cancelled)?;
    emit(
        app,
        &FirmwareState::Verifying {
            device_id: device_id.to_owned(),
        },
    );
    let rediscovered = rediscover(candidate, cancelled).ok_or_else(|| {
        InstallFailure::Verify("upload 후 Arduino를 다시 찾지 못했습니다.".to_owned())
    })?;
    ensure_not_cancelled(cancelled)?;
    let verified = probe_port(&rediscovered.port).map_err(InstallFailure::Verify)?;
    let _ = fs::remove_dir_all(output);
    if verified == HandshakeClassification::Installed {
        Ok(())
    } else {
        Err(InstallFailure::Verify(
            "업로드는 끝났지만 전용 펌웨어 확인에 실패했습니다.".to_owned(),
        ))
    }
}

#[tauri::command]
pub fn begin_firmware_install(
    app: AppHandle,
    device_id: String,
    installer: State<'_, FirmwareInstaller>,
    coordinator: State<'_, ArduinoCoordinator>,
) -> Result<(), String> {
    let candidate = installer.begin(&device_id).inspect_err(|error| {
        log_firmware(&format!(
            "[firmware] install rejected for {device_id}: {error}"
        ));
    })?;
    let installer = installer.inner().clone();
    let coordinator = coordinator.inner().clone();
    thread::spawn(move || {
        emit(
            &app,
            &FirmwareState::Preparing {
                device_id: device_id.clone(),
            },
        );
        let ownership = coordinator.acquire_installer();
        let result = match (candidate, ownership) {
            (candidate, Ok(ownership)) => {
                let result = install(&app, &device_id, &candidate, &installer.cancelled);
                let exit = match result {
                    Ok(()) => InstallerExit::Success,
                    Err(InstallFailure::Cancelled) => InstallerExit::Cancelled,
                    Err(_) => InstallerExit::Failure,
                };
                ownership.finish(exit);
                result
            }
            _ => Err(InstallFailure::Upload(
                "Arduino 포트를 사용할 수 없습니다.".to_owned(),
            )),
        };
        if let Err(error) = &result {
            log_firmware(&format!(
                "[firmware] install failed for {device_id}: {error:?}"
            ));
        }

        let state = match result {
            Ok(()) => FirmwareState::Complete { device_id },
            Err(InstallFailure::Cancelled) => FirmwareState::Cancelled,
            Err(InstallFailure::Upload(detail)) => FirmwareState::Error {
                code: "uploadFailed",
                retryable: true,
                detail: Some(detail),
            },
            Err(InstallFailure::Verify(detail)) => FirmwareState::Error {
                code: "verifyFailed",
                retryable: true,
                detail: Some(detail),
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
        let core_install = core_install_args();
        let strings = |args: &[OsString]| {
            args.iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(strings(&core_install), ["core", "install", "arduino:avr"]);
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
        let busy = probe_port("/dev/does-not-exist-hanbeon");
        assert!(
            busy.is_err(),
            "missing port must not look like a blank sketch"
        );
        let message = busy.unwrap_err().to_lowercase();
        assert!(
            message.contains("no such file")
                || message.contains("not found")
                || message.contains("couldn't open")
                || message.contains("no such device"),
            "open failure should keep the OS error: {message}"
        );

        let coordinator = ArduinoCoordinator::new(test_support::coordinator_probe_silent);
        {
            let _ownership = coordinator.acquire_installer().unwrap();
            assert_eq!(coordinator.owner(), ArduinoOwner::Installer);
        }
        assert_eq!(coordinator.owner(), ArduinoOwner::Connection);
    }

    #[test]
    fn startup_mode_opens_setup_for_supported_uno_without_hanbeon_firmware() {
        assert_eq!(
            startup_mode_for([]),
            StartupMode::Connect,
            "no Uno keeps normal discovery active"
        );
        assert_eq!(
            startup_mode_for([HandshakeClassification::Installed]),
            StartupMode::Connect
        );
        assert_eq!(
            startup_mode_for([HandshakeClassification::NoResponse]),
            StartupMode::Setup
        );
        assert_eq!(
            startup_mode_for([HandshakeClassification::DifferentFirmware]),
            StartupMode::Setup
        );
    }
}
