//! Sends scan-state LED feedback through the native Arduino transport.

use std::sync::Arc;

use crate::arduino::{self, OutputCommand};
use crate::scan::{Mode, Snapshot};

#[derive(Clone)]
pub struct LedBridge {
    enqueue: Arc<dyn Fn(OutputCommand) + Send + Sync>,
}

impl LedBridge {
    pub fn native_transport() -> Self {
        Self {
            enqueue: Arc::new(|command| {
                let _ = arduino::enqueue_output(command);
            }),
        }
    }

    #[cfg(test)]
    fn native(output: std::sync::mpsc::SyncSender<&'static [u8]>) -> Self {
        Self {
            enqueue: Arc::new(move |command| {
                let _ = output.try_send(command.bytes());
            }),
        }
    }

    pub fn sync(&self, snapshot: &Snapshot) {
        (self.enqueue)(command_for(snapshot));
    }

    pub fn for_worker(&self) -> Self {
        self.clone()
    }
}

fn command_for(snapshot: &Snapshot) -> OutputCommand {
    command_for_mode(snapshot.mode)
}

fn command_for_mode(mode: Mode) -> OutputCommand {
    match mode {
        Mode::Scanning => OutputCommand::Flash,
        Mode::Dwelling | Mode::Confirm | Mode::Paused => OutputCommand::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn snapshot(mode: Mode) -> Snapshot {
        Snapshot {
            cursor: 0,
            cells: vec![],
            preset: None,
            mode,
            interval_ms: 1800,
            phase_ms: 1800,
            remaining_ms: 1800,
        }
    }

    #[test]
    fn 순환_커서가_바뀌면_flash를_요청한다() {
        assert_eq!(command_for(&snapshot(Mode::Scanning)), OutputCommand::Flash);
    }

    #[test]
    fn 정지하면_led를_끄도록_요청한다() {
        assert_eq!(command_for(&snapshot(Mode::Paused)), OutputCommand::Off);
    }

    #[test]
    fn 머무름_중에는_led를_끄도록_요청한다() {
        assert_eq!(command_for(&snapshot(Mode::Dwelling)), OutputCommand::Off);
    }

    #[test]
    fn native_led_command_contract_maps_scanning_and_non_scanning_modes() {
        assert_eq!(command_for_mode(Mode::Scanning), OutputCommand::Flash);
        assert_eq!(command_for_mode(Mode::Dwelling), OutputCommand::Off);
        assert_eq!(command_for_mode(Mode::Confirm), OutputCommand::Off);
        assert_eq!(command_for_mode(Mode::Paused), OutputCommand::Off);
    }

    #[test]
    fn scanning_transition_enqueues_flash_to_native_writer() {
        let (tx, rx) = mpsc::sync_channel(8);
        let bridge = LedBridge::native(tx);
        bridge.sync(&snapshot(Mode::Scanning));
        assert_eq!(rx.try_recv(), Ok(b"FLASH\n".as_slice()));
    }

    #[test]
    fn native_writer_observes_exact_flash_off_bytes_in_order() {
        let (tx, rx) = mpsc::sync_channel(8);
        let bridge = LedBridge::native(tx);
        bridge.sync(&snapshot(Mode::Scanning));
        bridge.sync(&snapshot(Mode::Dwelling));
        bridge.sync(&snapshot(Mode::Confirm));
        bridge.sync(&snapshot(Mode::Paused));

        let mut bytes = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            bytes.extend_from_slice(chunk);
        }
        assert_eq!(bytes, b"FLASH\nOFF\nOFF\nOFF\n");
    }

    #[test]
    fn full_native_writer_drops_feedback_without_blocking() {
        let (tx, rx) = mpsc::sync_channel::<&'static [u8]>(1);
        tx.try_send(b"occupied").unwrap();
        let bridge = LedBridge::native(tx);
        bridge.sync(&snapshot(Mode::Scanning));
        assert_eq!(rx.try_recv(), Ok(b"occupied".as_slice()));
    }

    #[test]
    fn disconnected_native_writer_drops_off_without_blocking() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        let bridge = LedBridge::native(tx);
        bridge.sync(&snapshot(Mode::Paused));
    }
}
