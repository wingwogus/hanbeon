//! Transport-loss cancellation and suspension.
//!
//! These tests pin the core/input contract used by the Android JNI bridge:
//! a lost active source must never judge a held press, recovery may resume
//! only a transport-caused halt, and an explicit long-press pause persists.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hanbeon_core::action::Action;
use hanbeon_core::cue::Cue;
use hanbeon_core::gesture::{Gesture, GestureDetector, Judgement};
use hanbeon_core::host::{Host, HostError, Notice};
use hanbeon_core::journal::Journal;
use hanbeon_core::profile::{Profile, UndoMapping};
use hanbeon_core::scan::{Mode, Scanner};

struct RecordingHost {
    injects: Mutex<Vec<Action>>,
    undos: Mutex<usize>,
    cues: Mutex<Vec<Cue>>,
    modes: Mutex<Vec<Mode>>,
}

impl RecordingHost {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            injects: Mutex::new(Vec::new()),
            undos: Mutex::new(0),
            cues: Mutex::new(Vec::new()),
            modes: Mutex::new(Vec::new()),
        })
    }

    fn injects(&self) -> Vec<Action> {
        self.injects.lock().expect("inject log").clone()
    }

    fn last_mode(&self) -> Option<Mode> {
        self.modes.lock().expect("mode log").last().copied()
    }
}

impl Host for RecordingHost {
    fn inject(&self, action: Action) -> Result<(), HostError> {
        self.injects.lock().expect("inject log").push(action);
        Ok(())
    }

    fn undo(&self, _mapping: UndoMapping) -> Result<(), HostError> {
        *self.undos.lock().expect("undo log") += 1;
        Ok(())
    }

    fn open_settings(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn fit_cells(&self, _extras: usize) {}

    fn cue(&self, cue: Cue) {
        self.cues.lock().expect("cue log").push(cue);
    }

    fn set_sound(&self, _enabled: bool) {}

    fn publish(&self, notice: Notice) {
        if let Notice::State(snapshot) = notice {
            self.modes.lock().expect("mode log").push(snapshot.mode);
        }
    }

    fn save_profile(&self, _profile: &Profile) {}
}

fn fixed_profile() -> Profile {
    Profile {
        interval_ms: 1000,
        min_interval_ms: 1000,
        max_interval_ms: 1000,
        adaptive: false,
        ..Default::default()
    }
}

fn scanner() -> (Scanner, Arc<RecordingHost>) {
    let host = RecordingHost::new();
    let scanner = Scanner::new(
        Arc::new(Mutex::new(fixed_profile())),
        Arc::clone(&host) as Arc<dyn Host>,
        Journal::off(),
    );
    (scanner, host)
}

fn detector() -> GestureDetector {
    GestureDetector::new(Duration::from_millis(600))
}

/// Existing scanner: switch loss pauses and does not inject.
#[test]
fn transport_loss_characterization_switch_lost_pauses_without_injection() {
    let (scanner, host) = scanner();
    let before = scanner.snapshot().expect("snapshot");
    assert_eq!(before.mode, Mode::Scanning);
    assert_eq!(before.cursor, 0);

    scanner.switch_lost();

    let after = scanner.snapshot().expect("snapshot");
    assert_eq!(after.mode, Mode::Paused);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(after.interval_ms, before.interval_ms);
    assert!(
        host.injects().is_empty(),
        "switch_lost must not inject; got {:?}",
        host.injects()
    );
}

/// Existing detector: a completed press/release still judges. Loss must not
/// piggy-back on this path.
#[test]
fn transport_loss_characterization_completed_press_still_judges() {
    let mut detector = detector();
    let t0 = Instant::now();
    detector.on_press(t0);
    assert_eq!(
        detector
            .on_release(t0 + Duration::from_millis(200))
            .map(|judgement| judgement.gesture),
        Some(Gesture::Short)
    );
}

/// Existing detector: a completed press/release still judges if cancel is not
/// called. Disconnect must not piggy-back on this path.
#[test]
fn transport_loss_characterization_held_press_stays_pending() {
    let mut detector = detector();
    let t0 = Instant::now();
    detector.on_press(t0);
    assert_eq!(
        detector
            .on_release(t0 + Duration::from_millis(200))
            .map(|judgement| judgement.gesture),
        Some(Gesture::Short),
        "without cancel, a later release still judges"
    );
}

/// JNI `Core.suspendTransport()` / `Core.switchLost()`: drop the press, then halt.
fn cancel_and_suspend(detector: &mut GestureDetector, scanner: &Scanner) {
    detector.cancel();
    scanner.suspend_transport();
}

/// Held loss cancels with zero action. A later stale release cannot judge.
#[test]
fn transport_loss_while_held_cancels_with_zero_action() {
    let (scanner, host) = scanner();
    let mut detector = detector();
    let t0 = Instant::now();
    let before = scanner.snapshot().expect("snapshot");

    detector.on_press(t0);
    cancel_and_suspend(&mut detector, &scanner);

    let judgement = detector.on_release(t0 + Duration::from_millis(200));
    if let Some(judgement) = judgement {
        scanner.handle(judgement);
    }

    assert_eq!(
        judgement.map(|Judgement { gesture, .. }| gesture),
        None,
        "held transport loss must not produce a judgement"
    );
    assert!(
        host.injects().is_empty(),
        "held transport loss must yield zero action; got {:?}",
        host.injects()
    );
    let after = scanner.snapshot().expect("snapshot");
    assert_eq!(after.mode, Mode::Paused);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(host.last_mode(), Some(Mode::Paused));
}

/// Released loss enters transport suspension without moving the cursor.
#[test]
fn transport_loss_while_released_enters_transport_suspension() {
    let (scanner, host) = scanner();
    let mut detector = detector();
    let before = scanner.snapshot().expect("snapshot");

    cancel_and_suspend(&mut detector, &scanner);
    assert_eq!(detector.on_release(Instant::now()), None);

    let after = scanner.snapshot().expect("snapshot");
    assert_eq!(after.mode, Mode::Paused);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(after.interval_ms, before.interval_ms);
    assert!(host.injects().is_empty(), "released loss must not inject");
}

/// Recovery resumes the same cursor with a fresh full interval.
#[test]
fn transport_loss_recovery_resumes_same_cursor_full_interval() {
    let (scanner, host) = scanner();
    let mut detector = detector();
    let before = scanner.snapshot().expect("snapshot");

    cancel_and_suspend(&mut detector, &scanner);
    scanner.resume_transport();

    let after = scanner.snapshot().expect("snapshot");
    assert_eq!(after.mode, Mode::Scanning);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(after.interval_ms, before.interval_ms);
    assert_eq!(
        after.phase_ms, after.interval_ms,
        "resume must start a full scan interval, not a leftover dwell/confirm window"
    );
    assert!(
        after.remaining_ms >= after.interval_ms.saturating_sub(30),
        "resume remaining_ms must be a fresh interval, got {}",
        after.remaining_ms
    );
    assert!(host.injects().is_empty(), "resume must not inject");
}

/// An explicit long-press pause stays paused across loss and recovery.
#[test]
fn transport_loss_manual_pause_persists() {
    let (scanner, host) = scanner();
    let mut detector = detector();

    scanner.handle(Judgement {
        gesture: Gesture::Long,
        held: Duration::from_millis(700),
    });
    assert_eq!(scanner.snapshot().expect("snapshot").mode, Mode::Paused);

    cancel_and_suspend(&mut detector, &scanner);
    scanner.resume_transport();

    let after = scanner.snapshot().expect("snapshot");
    assert_eq!(after.mode, Mode::Paused);
    assert!(
        host.injects().is_empty(),
        "manual pause must not be turned into an action; got {:?}",
        host.injects()
    );
}

/// Adversarial: stale dwelling/confirm must not fire after resume.
#[test]
fn transport_loss_does_not_replay_stale_confirm_as_action() {
    let (scanner, host) = scanner();
    scanner.handle(Judgement {
        gesture: Gesture::Short,
        held: Duration::from_millis(120),
    });
    assert_eq!(host.injects(), vec![Action::Next]);

    let mut detector = detector();
    detector.on_press(Instant::now());
    cancel_and_suspend(&mut detector, &scanner);
    scanner.resume_transport();

    assert_eq!(
        scanner.snapshot().expect("snapshot").mode,
        Mode::Scanning,
        "resume must return to scanning, not the previous dwell/confirm deadline"
    );
    assert_eq!(host.injects(), vec![Action::Next]);
    assert_eq!(detector.on_release(Instant::now()), None);
}
