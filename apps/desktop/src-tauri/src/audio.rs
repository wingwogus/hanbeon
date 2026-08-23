//! 청각 피드백.
//!
//! 소음이 큰 환경과 저시력 상황 중 어느 하나만 가정하지 않는다(PRD 원칙 4).
//! 화면 강조만으로는 상태 변화를 놓치는 사용자가 있으므로 소리를 함께 낸다.
//!
//! 오디오 장치 핸들은 스레드 경계를 넘길 수 없어서, 전용 스레드가 장치를
//! 소유하고 채널로 신호만 받는다. 장치를 열지 못해도 앱은 그대로 동작한다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use rodio::Source;
use rodio::source::SineWave;

/// 서로 구분되어야 하는 신호음.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// 커서가 한 칸 이동했다.
    Tick,
    /// 동작을 실행했다.
    Select,
    /// 되돌리기 창에 들어갔거나 되돌렸다.
    Undo,
    /// 정지했다.
    Pause,
}

impl Cue {
    /// (주파수, 길이, 음량).
    ///
    /// 커서 이동은 초당 한 번꼴로 계속 나므로 짧고 작게 낸다. 되돌리기와 정지는
    /// 사용자가 반드시 알아차려야 하는 상태 변화라 낮고 길게 내 구분한다.
    fn tone(self) -> (f32, u64, f32) {
        match self {
            Cue::Tick => (660.0, 30, 0.06),
            Cue::Select => (940.0, 70, 0.12),
            Cue::Undo => (520.0, 150, 0.12),
            Cue::Pause => (330.0, 180, 0.10),
        }
    }
}

#[derive(Clone)]
pub struct Audio {
    tx: Option<Sender<Cue>>,
    /// 설정에서 끌 수 있다. 스레드는 그대로 두고 신호만 막는다.
    enabled: Arc<AtomicBool>,
}

impl Audio {
    /// 오디오 스레드를 띄운다. `HANBEON_SOUND=off`면 아무것도 하지 않는다.
    pub fn spawn() -> Self {
        if std::env::var("HANBEON_SOUND").is_ok_and(|value| value == "off") {
            return Self {
                tx: None,
                enabled: Arc::new(AtomicBool::new(false)),
            };
        }

        let (tx, rx) = mpsc::channel::<Cue>();

        thread::spawn(move || {
            let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(sink) => sink,
                Err(error) => {
                    eprintln!(
                        "소리 장치를 열지 못했습니다. 청각 피드백 없이 동작합니다. ({error})"
                    );
                    return;
                }
            };
            let mixer = sink.mixer();

            while let Ok(cue) = rx.recv() {
                let (hz, ms, amplitude) = cue.tone();
                mixer.add(
                    SineWave::new(hz)
                        .amplify(amplitude)
                        // 갑자기 시작하고 끊기면 딸깍 소리가 섞인다.
                        .fade_in(Duration::from_millis(5))
                        .take_duration(Duration::from_millis(ms)),
                );
            }
        });

        Self {
            tx: Some(tx),
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// 신호음을 낸다. 재생이 밀리더라도 스캔을 막지 않는다.
    pub fn play(&self, cue: Cue) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(cue);
        }
    }
}
