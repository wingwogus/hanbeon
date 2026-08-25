//! Native serial transport for the dedicated Hanbeon Arduino Uno switch.
//!
//! Discovery, handshake, P/R records, and reconnect live here. Press/release edges
//! enter the shared `GestureDetector` so HID/F13 fallback and native serial cannot
//! emit duplicate judgements. Accessibility is not used for this path.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::input::{Judgement, SharedDetector};

pub const BAUD_RATE: u32 = 115_200;
pub const HANDSHAKE_REQUEST: &[u8] = b"HELLO\n";
pub const HANDSHAKE_RESPONSE: &[u8] = b"HANBEON_UNO_V1\n";
pub const MAX_RECORD_BYTES: usize = 16;
/// Frontend listens on this event for waiting/connecting/connected/reconnecting/error.
pub const EVENT_LIFECYCLE: &str = "arduino://lifecycle";

const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(500);
const HANDSHAKE_ATTEMPTS: usize = 4;
const READ_TIMEOUT: Duration = Duration::from_millis(100);
const OUTPUT_QUEUE_CAPACITY: usize = 32;

type ActiveOutput = Option<(u64, SyncSender<OutputCommand>)>;

static ACTIVE_OUTPUT: OnceLock<Mutex<ActiveOutput>> = OnceLock::new();
static NEXT_OUTPUT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Lifecycle {
    Waiting,
    Connecting { port: String },
    Connected { port: String },
    Reconnecting,
    Error { message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchEvent {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputCommand {
    Flash,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    Io,
    InvalidHandshake,
    InvalidRecord,
    InvalidOutput,
}

/// Reject every response except the exact device identity record.
pub fn validate_handshake(response: &[u8]) -> Result<(), ProtocolError> {
    (response == HANDSHAKE_RESPONSE)
        .then_some(())
        .ok_or(ProtocolError::InvalidHandshake)
}

/// Send the identity request and validate its complete response.
pub fn perform_handshake<T: Read + Write + ?Sized>(serial: &mut T) -> Result<(), ProtocolError> {
    // Opening a USB Uno toggles DTR and resets it. Retry the exact request over
    // the bounded port timeout so a request lost during boot is harmless.
    for attempt in 0..HANDSHAKE_ATTEMPTS {
        serial
            .write_all(HANDSHAKE_REQUEST)
            .map_err(|_| ProtocolError::Io)?;
        serial.flush().map_err(|_| ProtocolError::Io)?;

        let mut response = [0; HANDSHAKE_RESPONSE.len()];
        match serial.read_exact(&mut response) {
            Ok(()) => return validate_handshake(&response),
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut && attempt + 1 < HANDSHAKE_ATTEMPTS => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(ProtocolError::InvalidHandshake);
            }
            Err(_) => return Err(ProtocolError::Io),
        }
    }

    Err(ProtocolError::InvalidHandshake)
}

/// Incrementally parses only newline-delimited `P` and `R` records.
#[derive(Default)]
pub struct RecordParser {
    pending: Vec<u8>,
    discarding_oversized_record: bool,
}

impl RecordParser {
    pub fn push(&mut self, input: &[u8]) -> Vec<Result<Record, ProtocolError>> {
        let mut records = Vec::new();

        for &byte in input {
            if self.discarding_oversized_record {
                if byte == b'\n' {
                    self.discarding_oversized_record = false;
                }
                continue;
            }

            if byte == b'\n' {
                let record = match self.pending.as_slice() {
                    b"P" | b"P\r" => Ok(Record::Press),
                    b"R" | b"R\r" => Ok(Record::Release),
                    _ => Err(ProtocolError::InvalidRecord),
                };
                records.push(record);
                self.pending.clear();
                continue;
            }

            self.pending.push(byte);
            if self.pending.len() > MAX_RECORD_BYTES {
                self.pending.clear();
                self.discarding_oversized_record = true;
                records.push(Err(ProtocolError::InvalidRecord));
            }
        }

        records
    }
}

impl TryFrom<&str> for OutputCommand {
    type Error = ProtocolError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "FLASH\n" => Ok(Self::Flash),
            "OFF\n" => Ok(Self::Off),
            _ => Err(ProtocolError::InvalidOutput),
        }
    }
}

impl OutputCommand {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Flash => b"FLASH\n",
            Self::Off => b"OFF\n",
        }
    }
}

/// Ensures disconnect is observable as one and only one release.
#[derive(Default)]
pub struct PressState {
    pressed: bool,
}

impl PressState {
    pub fn apply(&mut self, record: Record) -> Option<SwitchEvent> {
        match (self.pressed, record) {
            (false, Record::Press) => {
                self.pressed = true;
                Some(SwitchEvent::Pressed)
            }
            (true, Record::Release) => {
                self.pressed = false;
                Some(SwitchEvent::Released)
            }
            _ => None,
        }
    }

    pub fn disconnect(&mut self) -> Option<SwitchEvent> {
        self.pressed.then(|| {
            self.pressed = false;
            SwitchEvent::Released
        })
    }
}

/// Routes a native edge through the same gesture detector used by the HID fallback.
///
/// Duplicate serial records are filtered by `PressState` before this function runs.
/// The detector then ignores repeated presses, so HID/F13 and native serial cannot
/// both emit a judgement for the same physical actuation.
pub fn route_switch_event<F>(
    detector: &SharedDetector,
    event: SwitchEvent,
    now: Instant,
    on_gesture: F,
) where
    F: FnOnce(Judgement),
{
    let Ok(mut detector) = detector.lock() else {
        return;
    };

    match event {
        SwitchEvent::Pressed => detector.on_press(now),
        SwitchEvent::Released => {
            if let Some(judgement) = detector.on_release(now) {
                drop(detector);
                on_gesture(judgement);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconnectPolicy {
    pub retry_delay: Duration,
    pub max_candidates_per_cycle: usize,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            retry_delay: Duration::from_secs(2),
            max_candidates_per_cycle: 8,
        }
    }
}

impl ReconnectPolicy {
    pub fn candidates<'a>(&self, ports: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
        ports
            .into_iter()
            .take(self.max_candidates_per_cycle)
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum QueueError {
    Full,
    Stopped,
}

/// One-shot shutdown that remains requested after the channel value is consumed.
struct Shutdown {
    requested: AtomicBool,
    rx: Receiver<()>,
}

impl Shutdown {
    fn new(rx: Receiver<()>) -> Self {
        Self {
            requested: AtomicBool::new(false),
            rx,
        }
    }

    fn is_requested(&self) -> bool {
        if self.requested.load(Ordering::Acquire) {
            return true;
        }
        match self.rx.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => {
                self.requested.store(true, Ordering::Release);
                true
            }
            Err(TryRecvError::Empty) => false,
        }
    }
}

/// Handle for the background transport worker.
pub struct ArduinoSwitch {
    shutdown: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl ArduinoSwitch {
    /// Starts bounded serial discovery. Callbacks run on the transport worker.
    pub fn spawn<L, S>(policy: ReconnectPolicy, on_lifecycle: L, on_switch: S) -> Self
    where
        L: Fn(Lifecycle) + Send + 'static,
        S: Fn(SwitchEvent) + Send + 'static,
    {
        let (shutdown, shutdown_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(policy, Shutdown::new(shutdown_rx), on_lifecycle, on_switch)
        });

        Self {
            shutdown,
            worker: Some(worker),
        }
    }

    /// Queues only typed `FLASH`/`OFF` output without blocking the caller.
    pub fn enqueue(&self, command: OutputCommand) -> Result<(), QueueError> {
        enqueue_output(command)
    }

    /// Stops the worker and joins it so its serial port is closed before return.
    pub fn stop(mut self) {
        self.join_worker();
    }

    fn join_worker(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ArduinoSwitch {
    fn drop(&mut self) {
        self.join_worker();
    }
}

/// Queues LED feedback for the currently active native transport.
///
/// The registry lock protects only sender replacement. It is released before the
/// bounded, nonblocking queue operation so scanner state and serial I/O never nest.
pub fn enqueue_output(command: OutputCommand) -> Result<(), QueueError> {
    let output = ACTIVE_OUTPUT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| QueueError::Stopped)?
        .as_ref()
        .map(|(_, output)| output.clone())
        .ok_or(QueueError::Stopped)?;

    match output.try_send(command) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(QueueError::Full),
        Err(TrySendError::Disconnected(_)) => Err(QueueError::Stopped),
    }
}

struct OutputRegistration {
    id: u64,
}

impl OutputRegistration {
    fn unregister(&mut self) {
        unregister_output(self.id);
    }
}

impl Drop for OutputRegistration {
    fn drop(&mut self) {
        self.unregister();
    }
}

fn register_output(output: SyncSender<OutputCommand>) -> OutputRegistration {
    let id = NEXT_OUTPUT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut active) = ACTIVE_OUTPUT.get_or_init(|| Mutex::new(None)).lock() {
        *active = Some((id, output));
    }
    OutputRegistration { id }
}

fn unregister_output(id: u64) {
    if let Ok(mut active) = ACTIVE_OUTPUT.get_or_init(|| Mutex::new(None)).lock()
        && active
            .as_ref()
            .is_some_and(|(active_id, _)| *active_id == id)
    {
        *active = None;
    }
}

fn run_worker<L, S>(policy: ReconnectPolicy, shutdown: Shutdown, on_lifecycle: L, on_switch: S)
where
    L: Fn(Lifecycle),
    S: Fn(SwitchEvent),
{
    on_lifecycle(Lifecycle::Waiting);
    loop {
        if shutdown.is_requested() {
            return;
        }

        let ports = match serialport::available_ports() {
            Ok(ports) => ports,
            Err(error) => {
                on_lifecycle(Lifecycle::Error {
                    message: error.to_string(),
                });
                if wait_for_retry(&shutdown, policy.retry_delay) {
                    return;
                }
                continue;
            }
        };
        let port_names: Vec<_> = ports.iter().map(|port| port.port_name.as_str()).collect();
        let candidates = policy.candidates(port_names);

        if candidates.is_empty() {
            on_lifecycle(Lifecycle::Waiting);
            if wait_for_retry(&shutdown, policy.retry_delay) {
                return;
            }
            continue;
        }

        let mut connected = false;
        for port_name in candidates {
            if shutdown.is_requested() {
                return;
            }
            on_lifecycle(Lifecycle::Connecting {
                port: port_name.to_owned(),
            });

            let mut serial = match serialport::new(port_name, BAUD_RATE)
                .timeout(HANDSHAKE_TIMEOUT)
                .open()
            {
                Ok(serial) => serial,
                Err(_) => continue,
            };
            if perform_handshake(&mut *serial).is_err() {
                continue;
            }

            connected = true;
            let (output, output_rx) = mpsc::sync_channel(OUTPUT_QUEUE_CAPACITY);
            let output_registration = register_output(output);
            on_lifecycle(Lifecycle::Connected {
                port: port_name.to_owned(),
            });
            run_connection(&mut *serial, &output_rx, &shutdown, &on_switch);
            drop(output_registration);
            match lifecycle_after_connection(&shutdown) {
                Some(lifecycle) => on_lifecycle(lifecycle),
                None => return,
            }
        }

        if shutdown.is_requested() {
            return;
        }
        if !connected {
            on_lifecycle(Lifecycle::Reconnecting);
        }
        if wait_for_retry(&shutdown, policy.retry_delay) {
            return;
        }
    }
}

fn lifecycle_after_connection(shutdown: &Shutdown) -> Option<Lifecycle> {
    (!shutdown.is_requested()).then_some(Lifecycle::Reconnecting)
}

fn run_connection<S>(
    serial: &mut dyn serialport::SerialPort,
    output: &Receiver<OutputCommand>,
    shutdown: &Shutdown,
    on_switch: &S,
) where
    S: Fn(SwitchEvent),
{
    let _ = serial.set_timeout(READ_TIMEOUT);
    let mut parser = RecordParser::default();
    let mut press_state = PressState::default();
    let mut buffer = [0; 64];

    loop {
        if shutdown.is_requested() {
            if let Some(release) = press_state.disconnect() {
                on_switch(release);
            }
            return;
        }
        if !flush_output(serial, output) {
            break;
        }

        match serial.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                for record in parser.push(&buffer[..size]).into_iter().flatten() {
                    if let Some(event) = press_state.apply(record) {
                        if std::env::var("HANBEON_LOG").is_ok() {
                            eprintln!("[arduino] input: {event:?}");
                        }
                        on_switch(event);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }

    if let Some(release) = press_state.disconnect() {
        on_switch(release);
    }
}

fn flush_output(serial: &mut dyn Write, output: &Receiver<OutputCommand>) -> bool {
    loop {
        match output.try_recv() {
            Ok(command) => {
                if std::env::var("HANBEON_LOG").is_ok() {
                    eprintln!("[arduino] write: {command:?}");
                }
                if serial
                    .write_all(command.bytes())
                    .and_then(|()| serial.flush())
                    .is_err()
                {
                    return false;
                }
            }
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
}

fn wait_for_retry(shutdown: &Shutdown, retry_delay: Duration) -> bool {
    if shutdown.is_requested() {
        return true;
    }
    match shutdown.rx.recv_timeout(retry_delay) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {
            shutdown.requested.store(true, Ordering::Release);
            true
        }
        Err(RecvTimeoutError::Timeout) => shutdown.is_requested(),
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::sync::MutexGuard;

    static REGISTRY_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub struct RegistryGuard {
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for RegistryGuard {
        fn drop(&mut self) {
            if let Ok(mut active) = ACTIVE_OUTPUT.get_or_init(|| Mutex::new(None)).lock() {
                *active = None;
            }
        }
    }

    pub struct TestOutputRegistration(OutputRegistration);

    impl TestOutputRegistration {
        pub fn unregister(&mut self) {
            self.0.unregister();
        }
    }

    pub fn registry_guard() -> RegistryGuard {
        let guard = REGISTRY_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Ok(mut active) = ACTIVE_OUTPUT.get_or_init(|| Mutex::new(None)).lock() {
            *active = None;
        }
        RegistryGuard { _guard: guard }
    }

    pub fn register_test_output(
        capacity: usize,
    ) -> (TestOutputRegistration, Receiver<OutputCommand>) {
        let (output, output_rx) = mpsc::sync_channel(capacity);
        (TestOutputRegistration(register_output(output)), output_rx)
    }

    pub fn flush_output_to(serial: &mut dyn Write, output: &Receiver<OutputCommand>) -> bool {
        flush_output(serial, output)
    }

    pub fn shutdown_probe() -> (ArduinoSwitch, Receiver<()>) {
        let (shutdown, shutdown_rx) = mpsc::channel();
        let (consumed, consumed_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let shutdown = Shutdown::new(shutdown_rx);
            assert!(wait_for_retry(&shutdown, Duration::MAX));
            consumed.send(()).unwrap();
            assert!(wait_for_retry(&shutdown, Duration::from_secs(30)));
        });

        (
            ArduinoSwitch {
                shutdown,
                worker: Some(worker),
            },
            consumed_rx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HandshakeDouble {
        response: io::Cursor<Vec<u8>>,
        written: Vec<u8>,
        time_out_once: bool,
    }

    impl HandshakeDouble {
        fn with_response(response: &[u8]) -> Self {
            Self {
                response: io::Cursor::new(response.to_vec()),
                written: Vec::new(),
                time_out_once: false,
            }
        }
    }

    impl Read for HandshakeDouble {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.time_out_once {
                self.time_out_once = false;
                return Err(io::Error::new(io::ErrorKind::TimedOut, "Uno booting"));
            }
            self.response.read(output)
        }
    }

    impl Write for HandshakeDouble {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(input);
            Ok(input.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn handshake_accepts_only_hanbeon_uno_v1() {
        let mut device = HandshakeDouble::with_response(b"HANBEON_UNO_V1\n");
        assert_eq!(perform_handshake(&mut device), Ok(()));
        assert_eq!(device.written, HANDSHAKE_REQUEST);
        assert_eq!(
            validate_handshake(b"HANBEON_UNO_V1\r\n"),
            Err(ProtocolError::InvalidHandshake)
        );
    }

    #[test]
    fn handshake_retries_after_a_boot_timeout() {
        let mut device = HandshakeDouble::with_response(b"HANBEON_UNO_V1\n");
        device.time_out_once = true;
        assert_eq!(perform_handshake(&mut device), Ok(()));
        assert_eq!(
            device.written,
            [HANDSHAKE_REQUEST, HANDSHAKE_REQUEST].concat()
        );
    }

    #[test]
    fn handshake_rejects_unknown_device() {
        for response in [b"\n".as_slice(), b"OTHER_UNO\n", b"HANBEON_UNO_V2\n"] {
            let mut device = HandshakeDouble::with_response(response);
            assert_eq!(
                perform_handshake(&mut device),
                Err(ProtocolError::InvalidHandshake)
            );
            assert_eq!(device.written, HANDSHAKE_REQUEST);
        }
    }

    #[test]
    fn parser_accepts_crlf_press_release() {
        let mut parser = RecordParser::default();
        assert_eq!(
            parser.push(b"P\r\nR\n"),
            vec![Ok(Record::Press), Ok(Record::Release)]
        );
    }

    #[test]
    fn parser_rejects_malformed_and_multiple_records_safely() {
        let mut parser = RecordParser::default();
        assert_eq!(
            parser.push(b"P\nwrong\nR\n"),
            vec![
                Ok(Record::Press),
                Err(ProtocolError::InvalidRecord),
                Ok(Record::Release)
            ]
        );
        assert_eq!(
            parser.push(&[b'X'; MAX_RECORD_BYTES + 2]),
            vec![Err(ProtocolError::InvalidRecord)]
        );
        assert_eq!(parser.push(b"\nP\n"), vec![Ok(Record::Press)]);
    }

    #[test]
    fn disconnect_releases_pressed_state_once() {
        let mut state = PressState::default();
        assert_eq!(state.apply(Record::Press), Some(SwitchEvent::Pressed));
        assert_eq!(state.disconnect(), Some(SwitchEvent::Released));
        assert_eq!(state.disconnect(), None);
        assert_eq!(state.apply(Record::Release), None);
    }

    #[test]
    fn output_command_accepts_only_flash_off() {
        assert_eq!(OutputCommand::try_from("FLASH\n"), Ok(OutputCommand::Flash));
        assert_eq!(OutputCommand::try_from("OFF\n"), Ok(OutputCommand::Off));
        assert_eq!(
            OutputCommand::try_from("FLASH"),
            Err(ProtocolError::InvalidOutput)
        );
        assert_eq!(OutputCommand::Flash.bytes(), b"FLASH\n");
        assert_eq!(OutputCommand::Off.bytes(), b"OFF\n");
    }

    #[test]
    fn reconnect_policy_bounds_candidates() {
        let policy = ReconnectPolicy {
            retry_delay: Duration::from_secs(2),
            max_candidates_per_cycle: 2,
        };
        assert_eq!(
            policy.candidates(["one", "two", "three"]),
            vec!["one", "two"]
        );
    }

    #[test]
    fn shutdown_stays_requested_after_signal_is_consumed() {
        let (tx, rx) = mpsc::channel();
        let shutdown = Shutdown::new(rx);
        tx.send(()).unwrap();
        assert!(shutdown.is_requested());
        assert!(shutdown.is_requested());
    }

    #[test]
    fn wait_for_retry_returns_immediately_on_shutdown() {
        let (tx, rx) = mpsc::channel();
        let shutdown = Shutdown::new(rx);
        tx.send(()).unwrap();
        assert!(shutdown.is_requested());
        assert!(wait_for_retry(&shutdown, Duration::from_secs(30)));
    }

    #[test]
    fn shutdown_after_connection_skips_reconnecting() {
        let (tx, rx) = mpsc::channel();
        let shutdown = Shutdown::new(rx);
        tx.send(()).unwrap();
        assert_eq!(lifecycle_after_connection(&shutdown), None);
    }

    #[test]
    fn spawn_stop_joins_worker() {
        let (started_tx, started_rx) = mpsc::channel();
        let switch = ArduinoSwitch::spawn(
            ReconnectPolicy {
                retry_delay: Duration::from_secs(30),
                max_candidates_per_cycle: 8,
            },
            move |_| {
                let _ = started_tx.send(());
            },
            |_| {},
        );
        started_rx.recv().unwrap();
        switch.stop();
    }

    #[test]
    fn serial_press_release_delivers_one_short_judgement() {
        let detector = std::sync::Arc::new(Mutex::new(
            hanbeon_core::gesture::GestureDetector::new(Duration::from_millis(600)),
        ));
        let started = Instant::now();
        let mut parser = RecordParser::default();
        let mut state = PressState::default();
        let mut handled = Vec::new();

        for result in parser.push(b"P\nwrong\nP\nR\nR\n") {
            let Ok(record) = result else {
                continue;
            };
            if let Some(event) = state.apply(record) {
                let now = match event {
                    SwitchEvent::Pressed => started,
                    SwitchEvent::Released => started + Duration::from_millis(120),
                };
                route_switch_event(&detector, event, now, |judgement| handled.push(judgement));
            }
        }

        assert_eq!(handled.len(), 1, "scanner callback must run exactly once");
        assert_eq!(handled[0].gesture, crate::input::Gesture::Short);
        assert_eq!(handled[0].held, Duration::from_millis(120));
    }

    #[test]
    fn duplicate_press_and_disconnect_do_not_leave_detector_pressed() {
        let detector = std::sync::Arc::new(Mutex::new(
            hanbeon_core::gesture::GestureDetector::new(Duration::from_millis(600)),
        ));
        let started = Instant::now();
        let mut state = PressState::default();
        let mut handled = Vec::new();

        route_switch_event(
            &detector,
            state.apply(Record::Press).unwrap(),
            started,
            |judgement| handled.push(judgement),
        );
        assert_eq!(state.apply(Record::Press), None);
        route_switch_event(
            &detector,
            state.disconnect().unwrap(),
            started + Duration::from_millis(700),
            |judgement| handled.push(judgement),
        );
        assert_eq!(state.disconnect(), None);
        assert_eq!(state.apply(Record::Release), None);

        route_switch_event(
            &detector,
            state.apply(Record::Press).unwrap(),
            started + Duration::from_millis(800),
            |judgement| handled.push(judgement),
        );
        route_switch_event(
            &detector,
            state.apply(Record::Release).unwrap(),
            started + Duration::from_millis(900),
            |judgement| handled.push(judgement),
        );

        assert_eq!(handled.len(), 2, "forced release must leave detector free");
        assert_eq!(handled[0].gesture, crate::input::Gesture::Long);
        assert_eq!(handled[1].gesture, crate::input::Gesture::Short);
    }
}
