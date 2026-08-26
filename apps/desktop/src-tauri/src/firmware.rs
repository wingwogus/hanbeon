//! Registry-driven in-app firmware installer for supported boards.
//!
//! The bundled `arduino-cli` process is gone. Installation resolves the
//! connected board against Hana Cloud, downloads the SHA-256-verified
//! firmware, and flashes it over the optiboot bootloader directly.

use std::fs;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::arduino::{
    ArduinoCoordinator, BAUD_RATE, HANDSHAKE_REQUEST, HANDSHAKE_RESPONSE, InstallerExit,
};
use crate::flasher::{BootloaderFamily, FlashImage, SerialIo, family_for_fqbn, request_bootloader};
use crate::registry::{RegistryClient, RegistryError, UsbIdentity, VerifiedFirmware};

pub const EVENT_FIRMWARE: &str = "arduino://firmware";
/// ATmega328P의 SPM 페이지 크기(optiboot가 한 번에 쓰는 단위).
const FLASH_PAGE_BYTES: usize = 128;
/// Uno R3 bootloader 영역(512 bytes)을 제외한 애플리케이션 플래시 한계.
const UNO_APPLICATION_BYTES: u32 = 32_256;
/// optiboot 부팅 창을 몇 번까지 기다려 볼지. DTR 리셋 후 부트로더가 잠깐만
/// 열리므로 arduino-cli의 재시도 동작을 그대로 옮긴다.
const SYNC_ATTEMPTS: usize = 12;
const SERIAL_TIMEOUT: Duration = Duration::from_millis(500);
const REDISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const VERIFY_RETRY_WINDOW: Duration = Duration::from_secs(6);
const VERIFY_RETRY_INTERVAL: Duration = Duration::from_millis(700);
const REDISCOVERY_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArduinoCandidate {
    device_id: String,
    display_name: String,
    port: String,
    vid: u16,
    pid: u16,
    /// 레지스트리 매칭에 쓰는 USB descriptor 보조 문자열.
    #[serde(skip_serializing_if = "Option::is_none")]
    product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manufacturer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum FirmwareState {
    Searching,
    BoardFound {
        candidates: Vec<ArduinoCandidate>,
    },
    Probing {
        device_id: String,
    },
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

fn preferred_serial_path(name: &str) -> bool {
    !name.contains("/tty.") && !name.contains(r"\\.\\COM")
}

/// OS가 product 문자열을 주지 않거나("Generic CDC" 같은 일반 이름을 줄 때도
/// 있다) 의미 없는 이름일 때, VID/PID로 알려진 보드명을 되찾는다.
/// 레지스트리 매칭 전 단계의 표시용이며, 실제 등록 여부 판단은 여전히
/// 레지스트리 인덱스가 한다.
fn known_board_name(vid: u16, pid: u16) -> Option<&'static str> {
    match (vid, pid) {
        // Arduino Uno R3 계열(boards.txt 기준, 레지스트리 detect 항목과 동일)
        (0x2341, 0x0043)
        | (0x2341, 0x0001)
        | (0x2a03, 0x0043)
        | (0x2341, 0x0243)
        | (0x2341, 0x006a) => Some("Arduino Uno R3"),
        _ => None,
    }
}

fn serial_path_supported(port_name: &str, is_macos: bool) -> bool {
    !is_macos || port_name.contains("/cu.")
}

fn supported_candidate(port: serialport::SerialPortInfo) -> Option<ArduinoCandidate> {
    let serialport::SerialPortType::UsbPort(usb) = port.port_type else {
        return None;
    };
    if !serial_path_supported(&port.port_name, cfg!(target_os = "macos")) {
        return None;
    }

    // VID/PID로 후보를 좁히지 않는다. 어떤 보드가 연결됐는지는 레지스트리가
    // 판단하며, 등록되지 않은 VID/PID도 사용자 안내를 위해 목록에 남긴다.
    let generic_product = usb.product.as_deref().is_none_or(|product| {
        let lowered = product.to_ascii_lowercase();
        lowered.contains("generic") || lowered.contains("cdc") || lowered.trim().is_empty()
    });
    let display_name = match (&usb.product, generic_product) {
        (Some(product), false) => product.clone(),
        _ => known_board_name(usb.vid, usb.pid)
            .map(str::to_owned)
            .unwrap_or_else(|| "알 수 없는 시리얼 보드".to_owned()),
    };
    let product = if generic_product {
        None
    } else {
        usb.product.clone()
    };
    let device_id = usb.serial_number.as_deref().map_or_else(
        || format!("usb-{:04x}-{:04x}-{}", usb.vid, usb.pid, port.port_name),
        |serial| format!("usb-{:04x}-{:04x}-{serial}", usb.vid, usb.pid),
    );
    Some(ArduinoCandidate {
        device_id,
        display_name,
        port: port.port_name,
        vid: usb.vid,
        pid: usb.pid,
        product,
        manufacturer: usb.manufacturer,
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

fn usb_identity(candidate: &ArduinoCandidate) -> UsbIdentity {
    UsbIdentity {
        vid: candidate.vid,
        pid: candidate.pid,
        product: candidate.product.clone(),
        manufacturer: candidate.manufacturer.clone(),
    }
}

fn registered_candidates(
    app: &AppHandle,
    found: Vec<ArduinoCandidate>,
) -> Result<Vec<ArduinoCandidate>, String> {
    let cache_dir = registry_cache_dir(app)?;
    let index = RegistryClient::new(cache_dir)
        .load_index()
        .map_err(|error| error.to_string())?;
    Ok(found
        .into_iter()
        .filter(|candidate| {
            index
                .match_board(&usb_identity(candidate))
                .is_some_and(|matched| matched.confidence == crate::registry::Confidence::Exact)
        })
        .collect())
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
        let builder = serialport::new(port, BAUD_RATE).timeout(SERIAL_TIMEOUT);
        #[cfg(unix)]
        let builder = builder.exclusive(true);
        match builder.open() {
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
    let found = registered_candidates(&app, candidates()?).inspect_err(|error| {
        emit(
            &app,
            &FirmwareState::Error {
                code: "downloadFailed",
                retryable: true,
                detail: Some(error.clone()),
            },
        );
    })?;
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

#[tauri::command(rename_all = "camelCase")]
#[allow(non_snake_case)]
pub fn probe_arduino_firmware(
    app: AppHandle,
    deviceId: String,
    installer: State<'_, FirmwareInstaller>,
) -> Result<FirmwareState, String> {
    let device_id = deviceId;
    let candidate = find_candidate(&device_id)?;
    emit(
        &app,
        &FirmwareState::Probing {
            device_id: device_id.clone(),
        },
    );

    let found = registered_candidates(&app, vec![candidate.clone()])?;
    if found.is_empty() {
        return Err("Hana Cloud에 정확히 등록된 보드가 아닙니다.".to_owned());
    }
    // 빈 Uno는 전용 펌웨어가 없어 handshake에 답하지 않는다. 초기 식별 때
    // 포트를 열거나 요청을 보내지 않고, 사용자의 명시적 설치 확인만 받는다.
    installer.remember_confirmation(candidate.clone());
    let state = FirmwareState::ConfirmationRequired {
        device_id,
        reason: ConfirmationReason::NoResponse,
        display_name: candidate.display_name,
    };
    emit(&app, &state);
    Ok(state)
}

fn registry_cache_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|_| "앱 데이터 폴더를 찾지 못했습니다.".to_owned())?;
    Ok(base.join("hana-cloud"))
}

trait ResetLines {
    fn set_dtr(&mut self, level: bool) -> Result<(), String>;
    fn set_rts(&mut self, level: bool) -> Result<(), String>;
}

impl<T: serialport::SerialPort + ?Sized> ResetLines for T {
    fn set_dtr(&mut self, level: bool) -> Result<(), String> {
        self.write_data_terminal_ready(level)
            .map_err(|error| error.to_string())
    }

    fn set_rts(&mut self, level: bool) -> Result<(), String> {
        self.write_request_to_send(level)
            .map_err(|error| error.to_string())
    }
}

fn pulse_bootloader_reset<T: ResetLines + ?Sized>(
    lines: &mut T,
    mut delay: impl FnMut(Duration),
) -> Result<(), String> {
    lines.set_dtr(false)?;
    lines.set_rts(false)?;
    delay(Duration::from_millis(250));
    lines.set_dtr(true)?;
    lines.set_rts(true)?;
    delay(Duration::from_micros(100));
    lines.set_dtr(false)?;
    lines.set_rts(false)?;
    delay(Duration::from_millis(100));
    Ok(())
}

/// serialport 핸들을 플래셔의 SerialIo 트레잇에 맞춘다. 타임아웃 읽기는
/// TimedOut/WouldBlock 에러로 나타나며 read_response가 그것을 처리한다.
struct FlashPort {
    port: Box<dyn serialport::SerialPort>,
}

impl Read for FlashPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.port.read(buf)
    }
}

impl Write for FlashPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.port.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.port.flush()
    }
}

fn open_flash_port(port_name: &str) -> Result<FlashPort, String> {
    // exclusive 플래그는 쓰지 않는다. 설치 중 연결 스레드는 이미 멈춰 있고,
    // macOS에서 exclusive open이 간혹 EBUSY로 실패하는 것을 피한다.
    let mut port = serialport::new(port_name, BAUD_RATE)
        .timeout(SERIAL_TIMEOUT)
        .open()
        .map_err(|error| format!("{port_name}: {error}"))?;
    pulse_bootloader_reset(port.as_mut(), thread::sleep)
        .map_err(|error| format!("{port_name}: 보드를 리셋하지 못했습니다: {error}"))?;
    Ok(FlashPort { port })
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

use crate::flasher;

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
                        .find(|candidate| {
                            candidate.port == previous.port
                                && candidate.vid == previous.vid
                                && candidate.pid == previous.pid
                        })
                        .cloned()
                })
                .or_else(|| {
                    let mut same_model = found.iter().filter(|candidate| {
                        candidate.vid == previous.vid && candidate.pid == previous.pid
                    });
                    let only = same_model.next().cloned();
                    only.filter(|_| same_model.next().is_none())
                })
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
    /// 레지스트리 조회·다운로드 단계 실패.
    Download(String),
    Upload(String),
    Verify(String),
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), InstallFailure> {
    (!cancelled.load(Ordering::Acquire))
        .then_some(())
        .ok_or(InstallFailure::Cancelled)
}

/// 레지스트리에서 검증된 펌웨어를 가져온다. 사용자가 설치를 시작한 뒤에만
/// 호출된다(레지스트리 계약).
fn download_firmware(
    app: &AppHandle,
    candidate: &ArduinoCandidate,
    cancelled: &AtomicBool,
) -> Result<VerifiedFirmware, InstallFailure> {
    ensure_not_cancelled(cancelled)?;
    let cache_dir = registry_cache_dir(app).map_err(InstallFailure::Download)?;
    let client = RegistryClient::new(cache_dir);
    let matched = client
        .match_board(&usb_identity(candidate))
        .map_err(|error| match error {
            RegistryError::BoardNotRegistered { .. } => InstallFailure::Download(
                "이 보드는 한번 레지스트리에 등록되어 있지 않습니다.".to_owned(),
            ),
            other => InstallFailure::Download(other.to_string()),
        })?;
    log_firmware(&format!(
        "[firmware] matched {} ({:?})",
        matched.board_id, matched.confidence
    ));
    // exact가 아니면 자동 확정하지 않고 사용자 확인을 요구한다(레지스트리 계약).
    if matched.confidence != crate::registry::Confidence::Exact {
        return Err(InstallFailure::Download(format!(
            "보드 식별이 확실하지 않습니다({}). 보드가 정확한 모델인지 확인해 주세요.",
            matched.board_name
        )));
    }
    let firmware = client
        .resolve_firmware(&matched)
        .map_err(|error| match error {
            RegistryError::BoardNotRegistered { .. } => InstallFailure::Download(
                "이 보드는 한번 레지스트리에 등록되어 있지 않습니다.".to_owned(),
            ),
            other => InstallFailure::Download(other.to_string()),
        })?;
    Ok(firmware)
}

impl From<crate::flasher::FlashError> for InstallFailure {
    fn from(error: crate::flasher::FlashError) -> Self {
        match error {
            crate::flasher::FlashError::Cancelled => InstallFailure::Cancelled,
            other => InstallFailure::Upload(other.to_string()),
        }
    }
}

fn flash_with_retry(
    app: &AppHandle,
    candidate: &ArduinoCandidate,
    image: &FlashImage,
    hex_text: &str,
    family: BootloaderFamily,
    cancelled: &AtomicBool,
) -> Result<(), InstallFailure> {
    let mut last_error = String::new();
    let mut active_candidate = candidate.clone();
    for attempt in 0..3 {
        ensure_not_cancelled(cancelled)?;
        active_candidate = rediscover(&active_candidate, cancelled).ok_or_else(|| {
            InstallFailure::Upload("업로드할 Arduino를 다시 찾지 못했습니다.".to_owned())
        })?;
        if attempt > 0 {
            emit(
                app,
                &FirmwareState::Uploading {
                    device_id: candidate.device_id.clone(),
                },
            );
        }
        let flash_result = match family {
            BootloaderFamily::Stk500v1 => {
                let port = open_flash_port(&active_candidate.port)
                    .map_err(InstallFailure::Upload)?;
                let mut flash_port = Box::new(port) as Box<dyn SerialIo>;
                let mut written_bytes = 0usize;
                let result = flasher::synchronize(flash_port.as_mut(), SYNC_ATTEMPTS, cancelled)
                    .and_then(|()| {
                        flasher::program(
                            flash_port.as_mut(),
                            image,
                            FLASH_PAGE_BYTES,
                            cancelled,
                            &mut |written| {
                                // 페이지 경계마다만 기록하면 충분하다.
                                if written / FLASH_PAGE_BYTES != written_bytes / FLASH_PAGE_BYTES {
                                    written_bytes = written;
                                }
                            },
                        )
                    });
                drop(flash_port);
                result
            }
            BootloaderFamily::Uf2 => {
                // XIAO nRF52840의 부트로더는 UF2 드라이브가 아니라 시리얼 DFU
                // (adafruit-nrfutil)다. 부트로더 터치 후 nrfutil DFU 업로드.
                request_bootloader(&active_candidate.port)?;
                flasher::flash_serial_dfu(&active_candidate.port, hex_text)
            }
        };
        match flash_result {
            Ok(()) => return Ok(()),
            Err(flasher::FlashError::Cancelled) => return Err(InstallFailure::Cancelled),
            Err(error) => {
                last_error = error.to_string();
                log_firmware(&format!(
                    "[firmware] flash attempt {} failed: {last_error}",
                    attempt + 1
                ));
            }
        }
    }
    Err(InstallFailure::Upload(last_error))
}

fn install(
    app: &AppHandle,
    device_id: &str,
    candidate: &ArduinoCandidate,
    cancelled: &AtomicBool,
) -> Result<(), InstallFailure> {
    ensure_not_cancelled(cancelled)?;
    let firmware = download_firmware(app, candidate, cancelled)?;
    let image = FlashImage::from_ihex(&firmware.hex_text)
        .map_err(|error| InstallFailure::Download(format!("펌웨어 해석 실패: {error}")))?;
    let family = family_for_fqbn(&firmware.fqbn);

    ensure_not_cancelled(cancelled)?;
    emit(
        app,
        &FirmwareState::Uploading {
            device_id: device_id.to_owned(),
        },
    );
    flash_with_retry(
        app,
        candidate,
        &image,
        &firmware.hex_text,
        family,
        cancelled,
    )?;
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
    // 업로드 후 보드는 리셋되고 새 펌웨어가 부팅한다(1~2초). 첫 probe는
    // 부팅 중이라 실패할 수 있으므로, Installed가 확인될 때까지 여유를 둔다.
    let deadline = Instant::now() + VERIFY_RETRY_WINDOW;
    let mut verified = probe_port(&rediscovered.port).map_err(InstallFailure::Verify)?;
    while verified != HandshakeClassification::Installed
        && Instant::now() < deadline
        && !cancelled.load(Ordering::Acquire)
    {
        thread::sleep(VERIFY_RETRY_INTERVAL);
        match probe_port(&rediscovered.port) {
            Ok(classification) => verified = classification,
            Err(_) => continue,
        }
    }
    if verified == HandshakeClassification::Installed {
        Ok(())
    } else {
        Err(InstallFailure::Verify(
            "업로드는 끝났지만 전용 펌웨어 확인에 실패했습니다.".to_owned(),
        ))
    }
}

#[tauri::command(rename_all = "camelCase")]
#[allow(non_snake_case)]
pub fn begin_firmware_install(
    app: AppHandle,
    deviceId: String,
    installer: State<'_, FirmwareInstaller>,
    coordinator: State<'_, ArduinoCoordinator>,
) -> Result<(), String> {
    let device_id = deviceId;
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
            Err(InstallFailure::Download(detail)) => FirmwareState::Error {
                code: "downloadFailed",
                retryable: true,
                detail: Some(detail),
            },
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
    fn supported_device_filter_keeps_every_usb_serial_board() {
        // VID/PID로 미리 걸러내지 않는다. 등록 여부 판단은 레지스트리가 한다.
        let candidate = supported_candidate(usb_port(0x2341, 0x0043)).expect("uno kept");
        assert_eq!(candidate.vid, 0x2341);
        assert_eq!(candidate.product.as_deref(), Some("Uno"));
        let clone_board = supported_candidate(usb_port(0x1a86, 0x7523)).expect("ch340 kept");
        assert_eq!(clone_board.pid, 0x7523);
        assert!(
            supported_candidate(SerialPortInfo {
                port_name: "ttyS0".to_owned(),
                port_type: SerialPortType::Unknown,
            })
            .is_none(),
            "non-USB ports are not candidates"
        );
    }

    #[test]
    fn serial_path_filter_is_macos_only() {
        assert!(serial_path_supported("/dev/cu.usbmodem1", true));
        assert!(!serial_path_supported("/dev/tty.usbmodem1", true));
        assert!(serial_path_supported("/dev/ttyACM0", false));
        assert!(serial_path_supported("/dev/ttyUSB0", false));
        assert!(serial_path_supported("COM4", false));
    }

    #[test]
    fn generic_cdc_product_falls_back_to_known_board_name() {
        // macOS는 부팅 직후 product를 "Generic CDC"로 주기도 한다(ioreg 확인).
        let mut port = usb_port(0x2341, 0x0043);
        if let SerialPortType::UsbPort(ref mut usb) = port.port_type {
            usb.product = Some("Generic CDC".to_owned());
        }
        let candidate = supported_candidate(port).expect("uno kept");
        assert_eq!(candidate.display_name, "Arduino Uno R3");
        assert_eq!(
            candidate.product, None,
            "generic descriptors do not demote VID/PID matching"
        );

        // product가 아예 없어도 VID/PID로 되찾는다.
        let mut port = usb_port(0x2341, 0x0043);
        if let SerialPortType::UsbPort(ref mut usb) = port.port_type {
            usb.product = None;
            usb.manufacturer = None;
        }
        let candidate = supported_candidate(port).unwrap();
        assert_eq!(candidate.display_name, "Arduino Uno R3");

        // 등록 안 된 VID/PID + product 없음은 여전히 알 수 없음으로 남는다.
        let mut port = usb_port(0x1a86, 0x7523);
        if let SerialPortType::UsbPort(ref mut usb) = port.port_type {
            usb.product = None;
            usb.manufacturer = None;
        }
        let candidate = supported_candidate(port).unwrap();
        assert_eq!(candidate.display_name, "알 수 없는 시리얼 보드");
    }

    #[test]
    fn candidate_device_id_prefers_serial_number() {
        let mut port = usb_port(0x2341, 0x0043);
        let candidate = supported_candidate(port.clone()).unwrap();
        assert!(candidate.device_id.starts_with("usb-2341-0043-"));
        if let SerialPortType::UsbPort(ref mut usb) = port.port_type {
            usb.serial_number = Some("A123".to_owned());
        }
        let candidate = supported_candidate(port).unwrap();
        assert_eq!(candidate.device_id, "usb-2341-0043-A123");
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
        let coordinator = ArduinoCoordinator::new(test_support::coordinator_probe_silent);
        {
            let _ownership = coordinator.acquire_installer().unwrap();
            assert_eq!(coordinator.owner(), ArduinoOwner::Installer);
        }
        assert_eq!(coordinator.owner(), ArduinoOwner::Connection);
    }

    #[test]
    fn bootloader_reset_matches_avrdude_dtr_rts_sequence() {
        #[derive(Default)]
        struct FakeLines(Vec<(&'static str, bool)>);
        impl ResetLines for FakeLines {
            fn set_dtr(&mut self, level: bool) -> Result<(), String> {
                self.0.push(("dtr", level));
                Ok(())
            }

            fn set_rts(&mut self, level: bool) -> Result<(), String> {
                self.0.push(("rts", level));
                Ok(())
            }
        }

        let mut lines = FakeLines::default();
        let mut delays = Vec::new();
        pulse_bootloader_reset(&mut lines, |duration| delays.push(duration)).unwrap();
        assert_eq!(
            lines.0,
            vec![
                ("dtr", false),
                ("rts", false),
                ("dtr", true),
                ("rts", true),
                ("dtr", false),
                ("rts", false),
            ]
        );
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(250),
                Duration::from_micros(100),
                Duration::from_millis(100),
            ]
        );
    }

    #[test]
    fn firmware_state_serializes_device_id_as_camel_case() {
        let json = serde_json::to_value(FirmwareState::ConfirmationRequired {
            device_id: "candidate-1".to_owned(),
            reason: ConfirmationReason::NoResponse,
            display_name: "Arduino Uno".to_owned(),
        })
        .expect("serialize confirmation");
        assert_eq!(json["state"], "confirmationRequired");
        assert_eq!(json["deviceId"], "candidate-1");
        assert!(json.get("device_id").is_none());
    }
}
