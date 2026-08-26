//! In-app firmware flashing for the Arduino Uno R3 (optiboot bootloader).
//!
//! Replaces the bundled `arduino-cli` process with a direct implementation of
//! the two pieces the CLI provided:
//! - Intel HEX decoding (`ihex.rs` semantics, records type 00/01/04)
//! - The STK500v1 protocol optiboot speaks on the Uno's 16u2 at 115200 baud
//!
//! All sequencing is pure and unit-tested against a scripted fake serial port;
//! only the thin `SerialIo` impl touches real hardware.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Decoded flash image ready to hand to the STK500v1 uploader.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlashImage {
    /// Sparse page map keyed by word address (STK500 uses word addressing).
    pages: Vec<(u16, Vec<u8>)>,
    /// Sparse byte segments keyed by absolute flash address. Unlike STK500,
    /// UF2 uses byte addressing and can represent the full Intel HEX range.
    byte_segments: Vec<(u32, Vec<u8>)>,
}

impl FlashImage {
    pub fn from_ihex(hex_text: &str) -> Result<Self, String> {
        let mut bytes: Vec<(u32, u8)> = Vec::new();
        let mut upper = 0_u32;

        for (line_number, raw) in hex_text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let record = parse_ihex_record(line)
                .map_err(|error| format!("{line_number}번째 줄: {error}"))?;
            match record.record_type {
                0x00 => {
                    let base = upper.checked_add(u32::from(record.offset)).ok_or_else(|| {
                        format!("{line_number}번째 줄: 주소가 범위를 벗어났습니다")
                    })?;
                    for (index, byte) in record.data.iter().enumerate() {
                        let address = base.checked_add(index as u32).ok_or_else(|| {
                            format!("{line_number}번째 줄: 주소가 범위를 벗어났습니다")
                        })?;
                        bytes.push((address, *byte));
                    }
                }
                0x01 => break,
                0x04 => {
                    if record.data.len() != 2 {
                        return Err(format!("{line_number}번째 줄: 잘못된 확장 주소"));
                    }
                    upper = (u32::from(record.data[0]) << 24) | (u32::from(record.data[1]) << 16);
                }
                0x02 => {
                    if record.data.len() != 2 {
                        return Err(format!("{line_number}번째 줄: 잘못된 세그먼트 주소"));
                    }
                    upper = ((u32::from(record.data[0]) << 8) | u32::from(record.data[1])) << 4;
                }
                0x05 => {}
                other => {
                    return Err(format!(
                        "{line_number}번째 줄: 지원하지 않는 레코드 타입 {other:#x}"
                    ));
                }
            }
        }

        if bytes.is_empty() {
            return Err("펌웨어에 데이터가 없습니다".to_owned());
        }

        bytes.sort_by_key(|(address, _)| *address);
        let mut image = Self::default();
        let mut start = 0_usize;
        while start < bytes.len() {
            let chunk_end = bytes[start..]
                .windows(2)
                .take_while(|window| window[0].0 + 1 == window[1].0)
                .count()
                + 1;
            let chunk = &bytes[start..start + chunk_end];
            let byte_address = chunk[0].0;
            // STK500 addresses are in 16-bit words.
            if byte_address % 2 == 1 {
                return Err("홀수 주소에서 시작하는 데이터가 있습니다".to_owned());
            }
            let mut payload = Vec::with_capacity(chunk.len());
            for (_, byte) in chunk {
                payload.push(*byte);
            }
            image.byte_segments.push((byte_address, payload.clone()));
            // AVR의 STK500v1 address field is a 16-bit word address. Keep its
            // existing representation for that uploader, while preserving
            // high-address data above for byte-addressed UF2 targets.
            if let Ok(word_address) = u16::try_from(byte_address / 2) {
                image.pages.push((word_address, payload));
            }
            start += chunk_end;
        }
        Ok(image)
    }

    pub fn pages(&self) -> &[(u16, Vec<u8>)] {
        &self.pages
    }

    pub fn total_bytes(&self) -> usize {
        self.byte_segments.iter().map(|(_, data)| data.len()).sum()
    }

    pub fn highest_byte_address(&self) -> Option<u32> {
        self.pages
            .iter()
            .filter(|(_, data)| !data.is_empty())
            .map(|(word_address, data)| u32::from(*word_address) * 2 + data.len() as u32 - 1)
            .max()
    }

    pub fn fits_within(&self, byte_limit: u32) -> bool {
        self.highest_byte_address()
            .is_some_and(|address| address < byte_limit)
    }

    fn physical_pages(&self, page_size: usize) -> Result<Vec<(u16, Vec<u8>, usize)>, FlashError> {
        if page_size == 0 || !page_size.is_multiple_of(2) {
            return Err(FlashError::Protocol(
                "플래시 페이지 크기가 올바르지 않습니다".to_owned(),
            ));
        }
        let page_size_u32 = u32::try_from(page_size)
            .map_err(|_| FlashError::Protocol("플래시 페이지가 너무 큽니다".to_owned()))?;
        let mut pages: BTreeMap<u32, (Vec<Option<u8>>, usize)> = BTreeMap::new();
        for (word_address, data) in &self.pages {
            let start = u32::from(*word_address) * 2;
            for (offset, byte) in data.iter().copied().enumerate() {
                let address = start.checked_add(offset as u32).ok_or_else(|| {
                    FlashError::Protocol("펌웨어 주소가 범위를 벗어났습니다".to_owned())
                })?;
                let page_start = address / page_size_u32 * page_size_u32;
                let (page, source_bytes) = pages
                    .entry(page_start)
                    .or_insert_with(|| (vec![None; page_size], 0));
                let slot = &mut page[(address - page_start) as usize];
                if slot.replace(byte).is_some() {
                    return Err(FlashError::Protocol("펌웨어 주소가 중복됩니다".to_owned()));
                }
                *source_bytes += 1;
            }
        }

        pages
            .into_iter()
            .map(|(page_start, (page, source_bytes))| {
                let word_address = u16::try_from(page_start / 2).map_err(|_| {
                    FlashError::Protocol("펌웨어 주소가 범위를 벗어났습니다".to_owned())
                })?;
                Ok((
                    word_address,
                    page.into_iter().map(|byte| byte.unwrap_or(0xFF)).collect(),
                    source_bytes,
                ))
            })
            .collect()
    }

    fn byte_segments(&self) -> &[(u32, Vec<u8>)] {
        &self.byte_segments
    }
}

/// Bootloader transport selected by the board platform in its Arduino FQBN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootloaderFamily {
    Stk500v1,
    Uf2,
}

/// Selects the bootloader transport from the registry-provided FQBN.
///
/// Unknown platforms retain the established STK500v1 path so existing AVR
/// registry entries remain compatible; registry-supported nRF52 XIAO boards
/// explicitly select the UF2 path.
pub fn family_for_fqbn(fqbn: &str) -> BootloaderFamily {
    if fqbn.starts_with("Seeeduino:nrf52") {
        BootloaderFamily::Uf2
    } else {
        BootloaderFamily::Stk500v1
    }
}

const UF2_MAGIC_START0: u32 = 0x0A32_4655;
const UF2_MAGIC_START1: u32 = 0x9E5D_5157;
const UF2_FLAGS_FAMILY_ID_PRESENT: u32 = 0x0000_2000;
const UF2_PAYLOAD_SIZE: usize = 256;
const UF2_BLOCK_SIZE: usize = 512;
const UF2_MAGIC_END: u32 = 0x0AB1_6F30;

/// Encodes an Intel HEX image as UF2 blocks for a mass-storage bootloader.
///
/// The UF2 family ID occupies the standard header's final word when the
/// family-ID-present flag is set. Each block carries a fixed 256-byte payload
/// and targets the absolute byte address from the Intel HEX input.
pub fn encode_uf2(hex_image: &FlashImage, family_id: u32) -> Vec<u8> {
    let blocks: Vec<(u32, &[u8])> = hex_image
        .byte_segments()
        .iter()
        .flat_map(|(address, data)| {
            data.chunks(UF2_PAYLOAD_SIZE)
                .enumerate()
                .map(move |(index, payload)| {
                    (*address + (index * UF2_PAYLOAD_SIZE) as u32, payload)
                })
        })
        .collect();
    let total_blocks = blocks.len() as u32;
    let mut output = Vec::with_capacity(blocks.len() * UF2_BLOCK_SIZE);

    for (sequence, (address, payload)) in blocks.into_iter().enumerate() {
        let mut block = [0_u8; UF2_BLOCK_SIZE];
        let header = [
            UF2_MAGIC_START0,
            UF2_MAGIC_START1,
            UF2_FLAGS_FAMILY_ID_PRESENT,
            address,
            UF2_PAYLOAD_SIZE as u32,
            sequence as u32,
            total_blocks,
            family_id,
        ];
        for (index, word) in header.into_iter().enumerate() {
            block[index * 4..(index + 1) * 4].copy_from_slice(&word.to_le_bytes());
        }
        block[32..32 + payload.len()].copy_from_slice(payload);
        block[508..512].copy_from_slice(&UF2_MAGIC_END.to_le_bytes());
        output.extend_from_slice(&block);
    }
    output
}

/// Source of mounted volumes, isolated so UF2 writing is testable without a
/// physical board.
pub trait DriveProbe {
    fn volumes(&self) -> Vec<PathBuf>;
}

/// macOS volume probe for XIAO's TinyUSB mass-storage bootloader.
pub struct MacVolumes;

impl DriveProbe for MacVolumes {
    fn volumes(&self) -> Vec<PathBuf> {
        fs::read_dir("/Volumes")
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.to_ascii_uppercase().contains("XIAO"))
            })
            .collect()
    }
}

/// UF2 mass-storage uploader parameterized by its mounted-volume probe.
pub struct Uf2Flasher<P = MacVolumes> {
    probe: P,
}

impl<P: DriveProbe> Uf2Flasher<P> {
    pub fn new(probe: P) -> Self {
        Self { probe }
    }

    /// Writes the UF2 file into the one connected XIAO bootloader volume.
    pub fn flash(&self, uf2_bytes: &[u8]) -> Result<(), FlashError> {
        let volumes = self.probe.volumes();
        let volume = match volumes.as_slice() {
            [] => {
                return Err(FlashError::Uf2(
                    "XIAO UF2 부트로더 드라이브를 찾지 못했습니다.".to_owned(),
                ));
            }
            [volume] => volume,
            _ => {
                return Err(FlashError::Uf2(
                    "XIAO UF2 부트로더 드라이브가 여러 개 연결되어 있습니다.".to_owned(),
                ));
            }
        };
        fs::write(volume.join("hanbeon.uf2"), uf2_bytes)
            .map_err(|error| FlashError::Uf2(format!("UF2 파일을 쓰지 못했습니다: {error}")))
    }
}

impl Uf2Flasher<MacVolumes> {
    pub fn macos() -> Self {
        Self::new(MacVolumes)
    }
}

/// Seeed 코어가 번들한 adafruit-nrfutil 바이너리 경로를 찾는다.
fn find_adafruit_nrfutil() -> Result<PathBuf, FlashError> {
    // 우선순위: Arduino15 패키지 폴더(Seeed 코어 설치 시 자동 생성) -> PATH
    let home = std::env::var("HOME").unwrap_or_default();
    let bundled =
        PathBuf::from(home).join("Library/Arduino15/packages/Seeeduino/tools/adafruit-nrfutil");
    if let Ok(entries) = fs::read_dir(&bundled) {
        for entry in entries.flatten() {
            let macos = entry.path().join("macos/adafruit-nrfutil");
            if macos.is_file() {
                return Ok(macos);
            }
        }
    }
    // PATH에서도 탐색 (CLI 환경 대비)
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("adafruit-nrfutil");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(FlashError::Uf2(
        "adafruit-nrfutil을 찾지 못했습니다. Seeed nRF52 코어를 설치해 주세요.".to_owned(),
    ))
}

/// 부트로더에 시리얼 DFU로 Intel HEX 펌웨어를 올린다.
///
/// Seeed nRF52 코어가 사용하는 `adafruit-nrfutil` 호출과 같은 형식으로
/// HEX를 DFU zip으로 패키징한 뒤, 1200bps 터치로 진입한 부트로더에 올린다.
pub fn flash_serial_dfu(port_name: &str, hex_text: &str) -> Result<(), FlashError> {
    use std::process::Command;

    let nrfutil = find_adafruit_nrfutil()?;
    let work = std::env::temp_dir().join(format!("hanbeon-dfu-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)
        .map_err(|error| FlashError::Uf2(format!("작업 폴더 생성 실패: {error}")))?;

    let hex_path = work.join("firmware.hex");
    fs::write(&hex_path, hex_text)
        .map_err(|error| FlashError::Uf2(format!("HEX 쓰기 실패: {error}")))?;

    // 애플리케이션만 패키징한다. `0x0123`은 Seeed XIAO nRF52840(Sense)의
    // boards.txt에 정의된 S140 7.3.0 SoftDevice firmware ID다.
    let zip_path = work.join("firmware.zip");
    let package = Command::new(&nrfutil)
        .args([
            "dfu",
            "genpkg",
            "--dev-type",
            "0x0052",
            "--sd-req",
            "0x0123",
            "--application",
            &hex_path.to_string_lossy(),
            &zip_path.to_string_lossy(),
        ])
        .output()
        .map_err(|error| FlashError::Uf2(format!("nrfutil 실행 실패: {error}")))?;
    if !package.status.success() {
        return Err(FlashError::Uf2(format!(
            "DFU 패키징 실패: {}",
            String::from_utf8_lossy(&package.stderr).trim()
        )));
    }

    // 시리얼 DFU 업로드 (부트로더 모드 상태여야 한다)
    let upload = Command::new(&nrfutil)
        .args([
            "dfu",
            "serial",
            "-pkg",
            &zip_path.to_string_lossy(),
            "-p",
            port_name,
            "-b",
            "115200",
            "--singlebank",
        ])
        .output()
        .map_err(|error| FlashError::Uf2(format!("DFU 업로드 실행 실패: {error}")))?;
    let _ = fs::remove_dir_all(&work);
    if !upload.status.success() {
        return Err(FlashError::Uf2(format!(
            "DFU 업로드 실패: {}",
            String::from_utf8_lossy(&upload.stderr).trim()
        )));
    }
    Ok(())
}

pub fn request_bootloader(port_name: &str) -> Result<(), FlashError> {
    // TinyUSB CDC의 DFU 진입 조건은 3단계다: (1) 115200으로 CDC 세션 성립,
    // (2) 1200bps로 레이트 변경, (3) DTR을 내리며 닫기. 1200으로 바로 열면
    // line_coding 변경 이벤트가 없어 부트로더로 들어가지 않는다.
    for baud_rate in [115_200, 1200] {
        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(500))
            .open()
            .map_err(|error| {
                FlashError::Uf2(format!("부트로더 요청 포트를 열지 못했습니다: {error}"))
            })?;
        thread::sleep(Duration::from_millis(500));
        drop(port);
    }
    Ok(())
}

struct IhexRecord {
    record_type: u8,
    offset: u16,
    #[allow(dead_code)]
    checksum_valid: bool,
    data: Vec<u8>,
}

fn parse_ihex_record(line: &str) -> Result<IhexRecord, String> {
    let invalid = || "올바른 Intel HEX 레코드가 아닙니다".to_owned();
    if !line.starts_with(':') {
        return Err(invalid());
    }
    let body = &line[1..];
    if body.len() < 10 || !body.len().is_multiple_of(2) {
        return Err(invalid());
    }
    let decode_nibble = |c: char| c.to_digit(16).map(|d| d as u16).ok_or_else(invalid);
    let decode_pair = |pair: &str| -> Result<u16, String> {
        Ok(
            decode_nibble(pair.chars().next().ok_or_else(invalid)?)? * 16
                + decode_nibble(pair.chars().nth(1).ok_or_else(invalid)?)?,
        )
    };

    let count = decode_pair(&body[0..2])? as usize;
    if body.len() < 10 + count * 2 {
        return Err(invalid());
    }
    let offset = (decode_pair(&body[2..4])? << 8) | decode_pair(&body[4..6])?;
    let record_type = decode_pair(&body[6..8])? as u8;
    let mut data = Vec::with_capacity(count);
    for index in 0..count {
        let start = 8 + index * 2;
        data.push(decode_pair(&body[start..start + 2])? as u8);
    }
    let expected_checksum_index = 8 + count * 2;
    let stored = decode_pair(&body[expected_checksum_index..expected_checksum_index + 2])? as u8;
    // 모든 덧셈은 wrapping. 헤더 합도 255를 넘을 수 있다(고주소 레코드).
    let mut sum = (count as u8)
        .wrapping_add((offset >> 8) as u8)
        .wrapping_add(offset as u8)
        .wrapping_add(record_type);
    for byte in &data {
        sum = sum.wrapping_add(*byte);
    }
    let checksum_valid = sum.wrapping_add(stored) == 0;
    if !checksum_valid {
        return Err("체크섬이 일치하지 않습니다".to_owned());
    }
    Ok(IhexRecord {
        record_type,
        offset,
        checksum_valid,
        data,
    })
}

/// Byte-stream serial connection. Mirrors the subset of `Read + Write`
/// semantics used here so sessions can be tested offline; timeouts surface as
/// `TimedOut`/`WouldBlock` errors and are treated as "nothing arrived".
pub trait SerialIo: Read + Write {}

impl<T: Read + Write> SerialIo for T {}

/// Errors surfaced to the installer state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlashError {
    Sync,
    Protocol(String),
    Io(String),
    Uf2(String),
    Cancelled,
}

impl std::fmt::Display for FlashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sync => write!(
                f,
                "부트로더와 동기화하지 못했습니다. 보드의 리셋 버튼을 누른 뒤 다시 시도해 주세요."
            ),
            Self::Protocol(detail) => write!(f, "부트로더 응답이 잘못되었습니다: {detail}"),
            Self::Io(detail) => write!(f, "시리얼 통신 오류: {detail}"),
            Self::Uf2(detail) => write!(f, "UF2 업로드 오류: {detail}"),
            Self::Cancelled => write!(f, "설치가 취소되었습니다"),
        }
    }
}

const STK_GET_SYNC: u8 = 0x30;
const STK_ENTER_PROGMODE: u8 = 0x50;
const STK_LEAVE_PROGMODE: u8 = 0x51;
pub const STK_OK: u8 = 0x10;
const STK_INSYNC: u8 = 0x14;
const STK_CRC_EOP: u8 = 0x20;
const STK_CHIP_ERASE: u8 = 0x5D;
const STK_LOAD_ADDRESS: u8 = 0x55;
const STK_PROGRAM_PAGE: u8 = 0x64;
const STK_READ_SIGN: u8 = 0x75;
/// ATmega328P signature bytes, LSB-first as optiboot returns them.
const EXPECTED_SIGNATURE: [u8; 3] = [0x1E, 0x95, 0x0F];

fn frame(command: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 3);
    out.push(command);
    out.extend_from_slice(payload);
    out.push(STK_CRC_EOP);
    out
}

const COMMAND_TIMEOUT: Duration = Duration::from_millis(500);
const SYNC_TIMEOUT: Duration = Duration::from_millis(250);

fn read_response(
    port: &mut dyn SerialIo,
    expect_body: usize,
    timeout: Duration,
) -> Result<Vec<u8>, FlashError> {
    let deadline = Instant::now() + timeout;
    let mut buffer = Vec::with_capacity(expect_body + 2);
    let mut chunk = [0_u8; 64];
    while Instant::now() < deadline {
        match port.read(&mut chunk) {
            Ok(0) => return Err(FlashError::Io("포트가 닫혔습니다".to_owned())),
            Ok(size) => buffer.extend_from_slice(&chunk[..size]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(FlashError::Io(error.to_string())),
        }
        if buffer.len() >= expect_body + 2 {
            break;
        }
    }
    if buffer.first() != Some(&STK_INSYNC) {
        return Err(FlashError::Protocol("INSYNC 없음".to_owned()));
    }
    let tail = buffer.last().copied().unwrap_or(0);
    if tail != STK_OK {
        return Err(FlashError::Protocol(format!("STK_OK 대신 {tail:#x}")));
    }
    Ok(buffer[1..buffer.len().saturating_sub(1)].to_vec())
}

fn command(
    port: &mut dyn SerialIo,
    command_byte: u8,
    payload: &[u8],
    expect_body: usize,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, FlashError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(FlashError::Cancelled);
    }
    port.write_all(&frame(command_byte, payload))
        .and_then(|()| port.flush())
        .map_err(|e| FlashError::Io(e.to_string()))?;
    read_response(port, expect_body, COMMAND_TIMEOUT)
}

fn drain_input(port: &mut dyn SerialIo, quiet_for: Duration) {
    let deadline = Instant::now() + quiet_for;
    let mut scratch = [0_u8; 128];
    while Instant::now() < deadline {
        // 버리기 전용 읽기라 바이트 수는 중요하지 않다.
        let _ = port.read(&mut scratch);
    }
}

/// Sends GET_SYNC until the bootloader answers INSYNC/OK.
///
/// Optiboot listens for a moment after reset, then starts the sketch. Opening
/// the port toggles DTR and resets the board, so retrying across that boot
/// window is how arduino-cli's "double-tap" behavior is reproduced.
pub fn synchronize(
    port: &mut dyn SerialIo,
    attempts: usize,
    cancelled: &AtomicBool,
) -> Result<(), FlashError> {
    for attempt in 0..attempts {
        if cancelled.load(Ordering::Acquire) {
            return Err(FlashError::Cancelled);
        }
        let _ = port.write_all(&frame(STK_GET_SYNC, &[]));
        let _ = port.flush();
        if read_response(port, 0, SYNC_TIMEOUT).is_ok() {
            return Ok(());
        }
        // Bootloader may have timed out and jumped into the sketch; toggling
        // DTR by reopening happens one layer up. Here we just wait out the
        // bootloader restart window before trying again.
        let _ = attempt;
        thread::sleep(Duration::from_millis(80));
        drain_input(port, Duration::from_millis(20));
    }
    Err(FlashError::Sync)
}

/// Full optiboot programming session: enter, verify target, erase, write,
/// leave. Page writes use 128-byte pages (ATmega328P word-page 64 words).
pub fn program(
    port: &mut dyn SerialIo,
    image: &FlashImage,
    page_size: usize,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(usize),
) -> Result<(), FlashError> {
    let pages = image.physical_pages(page_size)?;
    command(port, STK_ENTER_PROGMODE, &[], 0, cancelled)?;
    let signature = command(port, STK_READ_SIGN, &[], 3, cancelled)?;
    if signature != EXPECTED_SIGNATURE {
        return Err(FlashError::Protocol(format!(
            "서명 불일치: {signature:02x?}"
        )));
    }
    command(port, STK_CHIP_ERASE, &[], 0, cancelled)?;

    let total = image.total_bytes();
    let mut written = 0usize;
    for (word_address, page, source_bytes) in pages {
        // Optiboot는 PROGRAM_PAGE마다 물리 페이지 전체를 지운다. 따라서 HEX의
        // sparse record를 먼저 정렬·병합하고, 정렬된 페이지를 정확히 한 번 쓴다.
        command(
            port,
            STK_LOAD_ADDRESS,
            &[(word_address & 0xFF) as u8, (word_address >> 8) as u8],
            0,
            cancelled,
        )?;
        // 페이지 길이는 [상위, 하위]; SPM_PAGESIZE≤255 기기에서 상위는
        // 버려지고 하위가 길이가 된다(optiboot GETLENGTH).
        let mut request = vec![(page.len() >> 8) as u8, page.len() as u8];
        request.extend_from_slice(b"F");
        request.extend_from_slice(&page);
        command(port, STK_PROGRAM_PAGE, &request, 0, cancelled)?;
        written += source_bytes;
        progress(written.min(total));
    }

    command(port, STK_LEAVE_PROGMODE, &[], 0, cancelled)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::mpsc;

    static TEMP_DIR_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    /// Scripted fake port: asserts every write matches an expectation and
    /// produces queued responses, like a mock server for the bootloader.
    struct FakePort {
        expected_writes: VecDeque<Vec<u8>>,
        responses: VecDeque<Vec<u8>>,
        reads: VecDeque<u8>,
        log: mpsc::Sender<String>,
    }

    impl FakePort {
        fn new(log: mpsc::Sender<String>) -> Self {
            Self {
                expected_writes: VecDeque::new(),
                responses: VecDeque::new(),
                reads: VecDeque::new(),
                log,
            }
        }

        fn expect(mut self, bytes: &[u8], response: &[u8]) -> Self {
            self.expected_writes.push_back(bytes.to_vec());
            self.responses.push_back(response.to_vec());
            self
        }
    }

    impl Write for FakePort {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let expected = self.expected_writes.pop_front().expect("unexpected write");
            assert_eq!(buf, expected.as_slice(), "frame mismatch");
            self.log.send(format!("W:{buf:02x?}")).unwrap();
            if let Some(response) = self.responses.pop_front() {
                self.reads.extend(response);
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for FakePort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.reads.is_empty() {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "empty"));
            }
            let mut count = 0;
            while count < buf.len()
                && let Some(byte) = self.reads.pop_front()
            {
                buf[count] = byte;
                count += 1;
            }
            Ok(count)
        }
    }

    const OK_RESPONSE: [u8; 2] = [STK_INSYNC, STK_OK];
    const SIGNATURE_RESPONSE: [u8; 5] = [STK_INSYNC, 0x1E, 0x95, 0x0F, STK_OK];

    struct FakeDriveProbe {
        volumes: Vec<PathBuf>,
    }

    impl DriveProbe for FakeDriveProbe {
        fn volumes(&self) -> Vec<PathBuf> {
            self.volumes.clone()
        }
    }

    fn test_dir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hanbeon-uf2-test-{tag}-{}-{}",
            std::process::id(),
            TEMP_DIR_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temporary volume");
        path
    }

    fn word(block: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(block[offset..offset + 4].try_into().expect("u32 word"))
    }

    #[test]
    fn family_for_fqbn_selects_stk500_and_uf2() {
        assert_eq!(
            family_for_fqbn("arduino:avr:uno"),
            BootloaderFamily::Stk500v1
        );
        assert_eq!(
            family_for_fqbn("Seeeduino:nrf52:xiaonRF52840Sense"),
            BootloaderFamily::Uf2
        );
        assert_eq!(
            family_for_fqbn("other:platform:board"),
            BootloaderFamily::Stk500v1
        );
    }

    #[test]
    fn encode_uf2_has_expected_headers_and_payloads() {
        let image = FlashImage::from_ihex(":020000040001F9\n:02000000AABB99\n:00000001FF\n")
            .expect("parse image");
        let encoded = encode_uf2(&image, 0x621E_11BE);
        assert_eq!(encoded.len(), UF2_BLOCK_SIZE);
        let block = &encoded[..UF2_BLOCK_SIZE];
        assert_eq!(word(block, 0), UF2_MAGIC_START0);
        assert_eq!(word(block, 4), UF2_MAGIC_START1);
        assert_eq!(word(block, 8), UF2_FLAGS_FAMILY_ID_PRESENT);
        assert_eq!(word(block, 12), 0x0001_0000);
        assert_eq!(word(block, 16), UF2_PAYLOAD_SIZE as u32);
        assert_eq!(word(block, 20), 0);
        assert_eq!(word(block, 24), 1);
        assert_eq!(word(block, 28), 0x621E_11BE);
        assert_eq!(&block[32..34], &[0xAA, 0xBB]);
        assert!(block[34..508].iter().all(|byte| *byte == 0));
        assert_eq!(word(block, 508), UF2_MAGIC_END);
    }

    #[test]
    fn uf2_flasher_writes_matched_volume_and_rejects_ambiguous_drives() {
        let volume = test_dir("matched");
        let flasher = Uf2Flasher::new(FakeDriveProbe {
            volumes: vec![volume.clone()],
        });
        flasher.flash(b"UF2").expect("write UF2");
        assert_eq!(
            fs::read(volume.join("hanbeon.uf2")).expect("read UF2"),
            b"UF2"
        );

        let zero = Uf2Flasher::new(FakeDriveProbe { volumes: vec![] });
        assert!(matches!(zero.flash(b"UF2"), Err(FlashError::Uf2(_))));

        let second = test_dir("second");
        let multiple = Uf2Flasher::new(FakeDriveProbe {
            volumes: vec![volume.clone(), second.clone()],
        });
        assert!(matches!(multiple.flash(b"UF2"), Err(FlashError::Uf2(_))));
        let _ = fs::remove_dir_all(volume);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn ihex_parses_data_eof_and_extended_addresses() {
        let hex = ":100000000C9434000C9451000C9451000C94510049\n:100010000C9451000C9451000C9451000C9451001C\n:020000020001FB\n:00000001FF\n";
        let image = FlashImage::from_ihex(hex).expect("parse");
        assert_eq!(image.pages().len(), 1, "contiguous lines merge");
        assert_eq!(image.total_bytes(), 32);

        // Extended segment 0x0001 shifts data to byte address 0x10.
        let hex_far = ":020000020001FB\n:100000000C9434000C9451000C9451000C94510049\n:00000001FF\n";
        let far = FlashImage::from_ihex(hex_far).unwrap();
        let (address, data) = far.pages()[0].clone();
        assert_eq!(address, 0x0008, "word address = segment*16/2");
        assert_eq!(data.len(), 16);
    }

    #[test]
    fn ihex_rejects_corrupt_records() {
        assert!(FlashImage::from_ihex("no colon\n").is_err());
        assert!(FlashImage::from_ihex(":00\n").is_err());
        // Bad checksum (last byte should be FF for EOF).
        assert!(FlashImage::from_ihex(":00000001FE\n").is_err());
        assert!(FlashImage::from_ihex("").is_err(), "empty file");
    }

    #[test]
    fn frames_match_stk500_wire_format() {
        assert_eq!(frame(STK_GET_SYNC, &[]), vec![0x30, STK_CRC_EOP]);
        let address_frame = frame(STK_LOAD_ADDRESS, &[0x00, 0x40]);
        assert_eq!(address_frame, vec![0x55, 0x00, 0x40, STK_CRC_EOP]);
        let page = frame(STK_PROGRAM_PAGE, &[0x00, 0x80, b'F', 0xAA]);
        assert_eq!(page, vec![0x64, 0x00, 0x80, b'F', 0xAA, STK_CRC_EOP]);
    }

    #[test]
    fn multi_chunk_page_advances_stk_address_per_chunk() {
        // 256바이트 연속 이미지: 128바이트 청크 2개. 두 번째 청크의 LOAD_ADDRESS는
        // word 주소가 64(128/2) 증가한 0x0040이어야 한다. 고정 주소를 반복 쓰면
        // 앞부분이 덮어써진다(실기기에서 펌웨어가 깨진 원인).
        let cancelled = AtomicBool::new(false);
        let (log_tx, _log_rx) = mpsc::channel();
        let data: Vec<u8> = (0..256).map(|i| i as u8).collect();
        // ihex 한 레코드는 최대 255바이트라 128바이트 2레코드로 구성한다.
        let record = ":80000000000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F202122232425262728292A2B2C2D2E2F303132333435363738393A3B3C3D3E3F404142434445464748494A4B4C4D4E4F505152535455565758595A5B5C5D5E5F606162636465666768696A6B6C6D6E6F707172737475767778797A7B7C7D7E7FC0\n:80008000808182838485868788898A8B8C8D8E8F909192939495969798999A9B9C9D9E9FA0A1A2A3A4A5A6A7A8A9AAABACADAEAFB0B1B2B3B4B5B6B7B8B9BABBBCBDBEBFC0C1C2C3C4C5C6C7C8C9CACBCCCDCECFD0D1D2D3D4D5D6D7D8D9DADBDCDDDEDFE0E1E2E3E4E5E6E7E8E9EAEBECEDEEEFF0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF40\n:00000001FF\n";
        let image = FlashImage::from_ihex(record).unwrap();
        assert_eq!(image.total_bytes(), 256);

        let expected_chunks: Vec<(u8, u8)> = vec![(0x00, 0x00), (0x40, 0x00)];
        let mut page_frames: Vec<Vec<u8>> = Vec::new();
        for (low, high) in &expected_chunks {
            let start = ((*high as usize) << 9) | ((*low as usize) << 1);
            let mut frame = vec![STK_PROGRAM_PAGE, 0x00, 0x80, b'F'];
            frame.extend_from_slice(&data[start..start + 128]);
            frame.push(STK_CRC_EOP);
            page_frames.push(frame);
        }

        let mut port = FakePort::new(log_tx)
            .expect(&[STK_ENTER_PROGMODE, STK_CRC_EOP], &OK_RESPONSE)
            .expect(&[STK_READ_SIGN, STK_CRC_EOP], &SIGNATURE_RESPONSE)
            .expect(&[STK_CHIP_ERASE, STK_CRC_EOP], &OK_RESPONSE)
            .expect(
                &[
                    STK_LOAD_ADDRESS,
                    expected_chunks[0].0,
                    expected_chunks[0].1,
                    STK_CRC_EOP,
                ],
                &OK_RESPONSE,
            )
            .expect(&page_frames[0], &OK_RESPONSE)
            .expect(
                &[
                    STK_LOAD_ADDRESS,
                    expected_chunks[1].0,
                    expected_chunks[1].1,
                    STK_CRC_EOP,
                ],
                &OK_RESPONSE,
            )
            .expect(&page_frames[1], &OK_RESPONSE)
            .expect(&[STK_LEAVE_PROGMODE, STK_CRC_EOP], &OK_RESPONSE);

        let result = program(&mut port, &image, 128, &cancelled, &mut |_| {});
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn full_program_session_matches_expected_frames() {
        let cancelled = AtomicBool::new(false);
        let (log_tx, _log_rx) = mpsc::channel();
        let image = FlashImage::from_ihex(":0400000012345678E8\n:00000001FF\n").unwrap();
        let mut program_frame = vec![STK_PROGRAM_PAGE, 0x00, 0x80, b'F', 0x12, 0x34, 0x56, 0x78];
        program_frame.resize(4 + 128, 0xFF);
        program_frame.push(STK_CRC_EOP);

        let mut port = FakePort::new(log_tx)
            .expect(&[STK_ENTER_PROGMODE, STK_CRC_EOP], &OK_RESPONSE)
            .expect(&[STK_READ_SIGN, STK_CRC_EOP], &SIGNATURE_RESPONSE)
            .expect(&[STK_CHIP_ERASE, STK_CRC_EOP], &OK_RESPONSE)
            .expect(&[STK_LOAD_ADDRESS, 0x00, 0x00, STK_CRC_EOP], &OK_RESPONSE)
            .expect(&program_frame, &OK_RESPONSE)
            .expect(&[STK_LEAVE_PROGMODE, STK_CRC_EOP], &OK_RESPONSE);

        let mut progress_calls = Vec::new();
        let result = program(&mut port, &image, 128, &cancelled, &mut |written| {
            progress_calls.push(written)
        });
        assert_eq!(result, Ok(()));
        assert_eq!(progress_calls, vec![4], "progress reported once per page");
    }

    #[test]
    fn wrong_signature_is_a_protocol_error() {
        let cancelled = AtomicBool::new(false);
        let (log_tx, _log_rx) = mpsc::channel();
        let image = FlashImage::from_ihex(":0400000012345678E8\n:00000001FF\n").unwrap();
        let bad_signature = vec![STK_INSYNC, 0x1E, 0x95, 0x11, STK_OK];
        let mut port = FakePort::new(log_tx)
            .expect(&[STK_ENTER_PROGMODE, STK_CRC_EOP], &OK_RESPONSE)
            .expect(&[STK_READ_SIGN, STK_CRC_EOP], &bad_signature);

        let result = program(&mut port, &image, 128, &cancelled, &mut |_| {});
        assert!(matches!(result, Err(FlashError::Protocol(message)) if message.contains("서명")));
        assert!(
            port.expected_writes.is_empty(),
            "signature mismatch must stop before erase"
        );
    }

    #[test]
    fn ihex_preserves_extended_linear_addresses_and_rejects_uno_overflow() {
        let far = FlashImage::from_ihex(":020000040001F9\n:020000001234B8\n:00000001FF\n")
            .expect("valid extended-linear-address file");
        assert_eq!(far.highest_byte_address(), Some(0x1_0001));
        assert!(!far.fits_within(32_256));
    }

    #[test]
    fn sparse_records_in_one_flash_page_are_assembled_once() {
        let image =
            FlashImage::from_ihex(":020000001234B8\n:02001000567820\n:00000001FF\n").unwrap();
        let pages = image.physical_pages(128).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].0, 0, "aligned STK word address");
        assert_eq!(pages[0].1.len(), 128);
        assert_eq!(&pages[0].1[0..2], &[0x12, 0x34]);
        assert!(pages[0].1[2..16].iter().all(|byte| *byte == 0xFF));
        assert_eq!(&pages[0].1[16..18], &[0x56, 0x78]);
        assert_eq!(pages[0].2, 4, "progress counts source bytes, not padding");
    }

    #[test]
    fn record_crossing_page_boundary_is_split_into_aligned_pages() {
        let image = FlashImage::from_ihex(":04007E00123456786A\n:00000001FF\n").unwrap();
        let pages = image.physical_pages(128).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].0, 0);
        assert_eq!(&pages[0].1[126..128], &[0x12, 0x34]);
        assert_eq!(pages[1].0, 64, "byte 128 is STK word address 64");
        assert_eq!(&pages[1].1[0..2], &[0x56, 0x78]);
        assert_eq!((pages[0].2, pages[1].2), (2, 2));
    }

    #[test]
    fn cancellation_stops_before_any_write() {
        let cancelled = AtomicBool::new(true);
        let (log_tx, _log_rx) = mpsc::channel();
        let image = FlashImage::from_ihex(":0400000012345678E8\n:00000001FF\n").unwrap();
        let mut port = FakePort::new(log_tx);
        let result = program(&mut port, &image, 128, &cancelled, &mut |_| {});
        assert_eq!(result, Err(FlashError::Cancelled));
    }

    #[test]
    fn missing_insync_reports_protocol_error() {
        let cancelled = AtomicBool::new(false);
        let (log_tx, log_rx) = mpsc::channel();
        let mut port = FakePort::new(log_tx);
        port.expected_writes.push_back(frame(STK_GET_SYNC, &[]));
        port.responses.push_back(vec![]);
        drop(std::thread::spawn(move || {
            while let Ok(line) = log_rx.recv_timeout(Duration::from_millis(100)) {
                let _ = line;
            }
        }));
        // No response queued: read_response times out without INSYNC.
        let result = synchronize(&mut port, 1, &cancelled);
        assert_eq!(result, Err(FlashError::Sync));
    }
}
