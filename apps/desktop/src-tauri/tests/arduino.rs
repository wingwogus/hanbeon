#[allow(dead_code)]
#[path = "../src/arduino.rs"]
mod arduino;
#[allow(dead_code)]
#[path = "../src/input.rs"]
mod input;

use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use arduino::{PressState, Record, SwitchEvent};
use input::{Gesture, GestureDetector, Judgement};

fn assert_shutdown_joins(action: impl FnOnce(arduino::ArduinoSwitch) + Send + 'static) {
    let (switch, consumed) = arduino::test_support::shutdown_probe();
    let (joined, joined_rx) = mpsc::channel();
    let shutdown_thread = thread::spawn(move || {
        action(switch);
        joined.send(()).unwrap();
    });

    consumed.recv_timeout(Duration::from_secs(1)).unwrap();
    joined_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    shutdown_thread.join().unwrap();
}

#[test]
fn spawn_stop_joins_after_shutdown_was_consumed() {
    assert_shutdown_joins(arduino::ArduinoSwitch::stop);
}

#[test]
fn spawn_drop_joins_after_shutdown_was_consumed() {
    assert_shutdown_joins(drop);
}

fn route_at(
    detector: &Arc<Mutex<GestureDetector>>,
    event: SwitchEvent,
    now: Instant,
    handled: &mut Vec<Judgement>,
) {
    arduino::route_switch_event(detector, event, now, |judgement| handled.push(judgement));
}

#[test]
fn serial_press_release_delivers_one_short_judgement() {
    let detector = Arc::new(Mutex::new(GestureDetector::new(Duration::from_millis(600))));
    let started = Instant::now();
    let mut parser = arduino::RecordParser::default();
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
            route_at(&detector, event, now, &mut handled);
        }
    }

    assert_eq!(handled.len(), 1, "scanner callback must run exactly once");
    assert_eq!(handled[0].gesture, Gesture::Short);
    assert_eq!(handled[0].held, Duration::from_millis(120));
}

#[test]
fn duplicate_press_and_disconnect_do_not_leave_detector_pressed() {
    let detector = Arc::new(Mutex::new(GestureDetector::new(Duration::from_millis(600))));
    let started = Instant::now();
    let mut state = PressState::default();
    let mut handled = Vec::new();

    route_at(
        &detector,
        state.apply(Record::Press).unwrap(),
        started,
        &mut handled,
    );
    assert_eq!(
        state.apply(Record::Press),
        None,
        "duplicate press must not create another detector transition"
    );
    route_at(
        &detector,
        state.disconnect().unwrap(),
        started + Duration::from_millis(700),
        &mut handled,
    );
    assert_eq!(state.disconnect(), None, "disconnect releases only once");
    assert_eq!(
        state.apply(Record::Release),
        None,
        "stale release is ignored"
    );

    route_at(
        &detector,
        state.apply(Record::Press).unwrap(),
        started + Duration::from_millis(800),
        &mut handled,
    );
    route_at(
        &detector,
        state.apply(Record::Release).unwrap(),
        started + Duration::from_millis(900),
        &mut handled,
    );

    assert_eq!(handled.len(), 2, "forced release must leave detector free");
    assert_eq!(handled[0].gesture, Gesture::Long);
    assert_eq!(handled[1].gesture, Gesture::Short);
}

#[test]
fn fake_native_writer_observes_exact_flash_off_bytes() {
    let (tx, rx) = mpsc::sync_channel(8);
    tx.try_send(arduino::OutputCommand::Flash).unwrap();
    tx.try_send(arduino::OutputCommand::Off).unwrap();
    let mut serial = Vec::new();
    assert!(
        arduino::test_support::flush_output_to(&mut serial, &rx),
        "native writer should stay open after draining FLASH/OFF"
    );
    assert_eq!(serial, b"FLASH\nOFF\n");
}

#[test]
fn disconnected_commands_are_not_replayed_after_registration() {
    let _registry = arduino::test_support::registry_guard();
    let (old_connection, _old_rx) = arduino::test_support::register_test_output(8);
    arduino::enqueue_output(arduino::OutputCommand::Flash).unwrap();
    drop(old_connection);

    assert_eq!(
        arduino::enqueue_output(arduino::OutputCommand::Flash),
        Err(arduino::QueueError::Stopped)
    );

    let (_new_connection, new_rx) = arduino::test_support::register_test_output(8);
    arduino::enqueue_output(arduino::OutputCommand::Off).unwrap();
    let mut serial = Vec::new();
    assert!(arduino::test_support::flush_output_to(&mut serial, &new_rx));
    assert_eq!(serial, b"OFF\n");
}

#[test]
fn production_registry_full_queue_does_not_block() {
    let _registry = arduino::test_support::registry_guard();
    let (_connection, rx) = arduino::test_support::register_test_output(1);
    arduino::enqueue_output(arduino::OutputCommand::Flash).unwrap();
    assert_eq!(
        arduino::enqueue_output(arduino::OutputCommand::Off),
        Err(arduino::QueueError::Full)
    );
    assert_eq!(rx.try_recv(), Ok(arduino::OutputCommand::Flash));
}

#[test]
fn production_registry_receiver_replacement_remains_valid() {
    let _registry = arduino::test_support::registry_guard();
    let (old_connection, old_rx) = arduino::test_support::register_test_output(1);
    let (_new_connection, new_rx) = arduino::test_support::register_test_output(1);
    drop(old_rx);
    drop(old_connection);

    arduino::enqueue_output(arduino::OutputCommand::Off).unwrap();
    assert_eq!(new_rx.try_recv(), Ok(arduino::OutputCommand::Off));
}

#[test]
fn stale_output_handle_cannot_unregister_newer_writer() {
    let _registry = arduino::test_support::registry_guard();
    let (mut old_connection, _old_rx) = arduino::test_support::register_test_output(1);
    let (_new_connection, new_rx) = arduino::test_support::register_test_output(1);
    old_connection.unregister();

    arduino::enqueue_output(arduino::OutputCommand::Flash).unwrap();
    assert_eq!(new_rx.try_recv(), Ok(arduino::OutputCommand::Flash));
}
