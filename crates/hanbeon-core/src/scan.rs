//! 스캔 상태기계와 주사 타이머.
//!
//! ```text
//!    ┌──────────┐  짧게 누름   ┌──────────┐  머무름 만료
//!    │ SCANNING ├─────────────►│ DWELLING ├──────────────┐
//!    │ (순환)   │◄─────────────┤ (머무름) │              │
//!    └────┬─────┘              └────┬─────┘◄─────────────┘
//!         │ 길게 누름               │ 짧게 누름 = 같은 동작 반복
//!         ▼                         │ Enter 실행 시
//!    ┌──────────┐              ┌────▼─────┐
//!    │ PAUSED   │              │ CONFIRM  │ 짧게 누름 = 되돌리기
//!    │ (정지)   │              │ (3초)    │ 만료 = DWELLING
//!    └──────────┘              └──────────┘
//! ```
//!
//! 상태 전이(`State`)와 부수효과(`Command` 실행)를 분리한다. 전이는 시각을
//! 인자로 받는 순수 로직이라 단위 테스트로 고정할 수 있고, 키 주입은 락을
//! 놓은 뒤에 실행할 수 있다.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::action::{Action, Cell, Kind};
use crate::adapt::{Adapter, Adjustment, Limits, Sample};
use crate::cue::Cue;
use crate::gesture::{Gesture, Judgement};
use crate::host::{Host, Notice};
use crate::journal::{Event, Journal};
use crate::profile::{Profile, UndoMapping};

/// 선택 직후 되돌릴 수 있는 시간.
const CONFIRM_WINDOW: Duration = Duration::from_millis(3000);

/// 머무름 유지 시간은 주사 간격의 이 배수다.
/// 1.0이면 순환과 구분이 안 되고, 너무 길면 다른 동작으로 넘어가기 어렵다.
const DWELL_FACTOR: u32 = 3;
const DWELL_DIVISOR: u32 = 2;

/// 상태 확인 주기. 주사 간격 오차 예산(±30ms)보다 충분히 짧다.
const TICK: Duration = Duration::from_millis(10);

/// 이 바퀴 수를 넘도록 누르지 않으면 그때까지 지나온 자리는 놓침으로 세지 않는다.
/// 놓침 한 번의 값이 한 바퀴이므로 여유를 한 바퀴 더 준다.
const IDLE_CYCLES: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// 커서가 순환 중
    Scanning,
    /// 실행한 칸에 머무는 중. 다시 누르면 같은 동작을 반복한다.
    Dwelling,
    /// 되돌리기 창. 누르면 직전 선택을 되돌린다.
    Confirm,
    /// 정지. 어떤 동작도 실행하지 않는다.
    Paused,
}

impl Mode {
    /// 기록에 남길 이름. 코어의 타입을 그대로 적으면 타입이 바뀔 때마다
    /// 지난 기록을 읽을 수 없게 된다.
    fn as_str(self) -> &'static str {
        match self {
            Mode::Scanning => "scanning",
            Mode::Dwelling => "dwelling",
            Mode::Confirm => "confirm",
            Mode::Paused => "paused",
        }
    }
}

/// 상태기계가 바깥에 요청하는 부수효과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Emit(Action),
    Undo,
    OpenSettings,
}

/// 화면이 그릴 칸 하나. 무엇을 보내는 칸인지는 알려주지 않는다.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellView {
    pub label: String,
    pub name: String,
    /// 화면이 배치와 생김새를 정하는 근거. 앱별 칸은 여기서 갈린다.
    pub kind: Kind,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub cursor: usize,
    /// 지금 도는 칸 전체. 앱에 따라 4~7칸이다.
    pub cells: Vec<CellView>,
    /// 붙어 있는 앱별 프리셋 이름. 없으면 앞 4칸만 돈다.
    pub preset: Option<String>,
    pub mode: Mode,
    pub interval_ms: u64,
    /// 지금 모드가 통째로 지속되는 시간. 남은 시간 표시의 분모다.
    ///
    /// 모드마다 길이가 다르다 — 순환은 주사 간격, 머무름은 그 1.5배,
    /// 되돌리기는 3초. 프론트가 주사 간격만 알면 머무름·되돌리기에서
    /// 눈금이 실제 마감과 어긋난다.
    pub phase_ms: u64,
    /// 이 모드가 끝나기까지 남은 시간. 남은 시간 표시의 분자다.
    pub remaining_ms: u64,
}

/// 짧게 누름의 결과. 실행할 동작과, 그로 인한 간격 조정.
///
/// 반응시간과 지나온 자리 수를 함께 들고 나온다. 실행한 뒤에는 이미 상태가
/// 바뀌어 있어서, 그때 가서는 '누르기까지 얼마나 걸렸는지'를 알 수 없다.
#[derive(Debug, Default)]
struct Outcome {
    command: Option<Command>,
    adjustment: Option<Adjustment>,
    /// 커서가 그 칸에 들어온 뒤 누르기까지.
    ///
    /// 순환 중에 고른 경우에만 값이 있다. 머무름 연타에는 '커서 진입'이라는 것이
    /// 없어서 이 값이 정의되지 않는다(PRD F5의 `t_react` 정의). 그 자리에 누적된
    /// 값을 적으면 반응속도 지표가 통째로 부풀려진다.
    reaction_ms: Option<u64>,
    /// 지난 선택 이후 커서가 지나온 자리 수.
    steps: u32,
    /// 그때 순환에 있던 자리 수.
    cycle: usize,
    /// 누른 칸의 이름.
    cell: String,
    /// 누를 때의 모드. 순환 중 선택과 머무름 연타를 나중에 갈라 세기 위한 것이다.
    mode: &'static str,
}

struct State {
    /// 순환에서 지금 몇 번째 자리인지. 칸 번호가 아니다 —
    /// `Enter`는 칸이 하나여도 순환에는 두 번 나온다.
    position: usize,
    /// 지금 도는 칸.
    cells: Vec<Cell>,
    /// 자리 → 칸 번호.
    order: Vec<usize>,
    /// 다음에 자리를 옮길 때 처음으로 돌아갈지.
    ///
    /// 선택한 직후에는 다시 옮기는 일이 가장 잦다. 그래서 `Enter`를 누르면
    /// 이어서 순환하는 대신 첫 이동 칸부터 다시 시작한다.
    restart_next: bool,
    /// 표시 이름과 별개인 프리셋 버전 키.
    preset_key: Option<String>,
    preset: Option<String>,
    /// 외부 registry 인식 완료 문구는 세션에 ID당 한 번만 알린다.
    recognized_presets: HashSet<String>,
    mode: Mode,
    interval: Duration,
    /// 현재 모드가 끝나는 시각.
    deadline: Instant,
    /// 현재 모드가 시작될 때 잡힌 길이. `deadline`과 짝으로만 움직인다.
    phase: Duration,
    /// 커서가 지금 칸에 들어온 시각. 반응시간을 재는 기준이다.
    entered_at: Instant,
    /// 마지막 선택 이후 커서가 몇 칸 움직였는지.
    /// 한 바퀴(4칸)를 채웠다면 원하는 칸을 지나쳐 다시 기다린 것이다.
    steps: u32,
    /// 마지막으로 사용자가 누른 시각. 쉬고 있던 시간을 놓침에서 걸러 내는 데 쓴다.
    last_input_at: Instant,
    adapter: Adapter,
    limits: Limits,
    adaptation_active: bool,
    /// Why scanning is halted. `Paused` is shared by a long-press stop and a
    /// missing switch; recovery may resume only a transport halt.
    pause_reason: Option<PauseReason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseReason {
    User,
    Transport,
}

impl State {
    fn new(profile: &Profile, now: Instant) -> Self {
        let interval = Duration::from_millis(profile.interval_ms);
        let cells = crate::action::base_cells();
        let order = crate::action::scan_order(&cells);

        Self {
            position: 0,
            cells,
            order,
            restart_next: false,
            preset_key: None,
            preset: None,
            recognized_presets: HashSet::new(),
            mode: Mode::Scanning,
            interval,
            deadline: now + interval,
            phase: interval,
            entered_at: now,
            steps: 0,
            last_input_at: now,
            adapter: Adapter::default(),
            limits: Limits {
                min_ms: profile.min_interval_ms,
                max_ms: profile.max_interval_ms,
            },
            adaptation_active: profile.adaptation_active(),
            pause_reason: None,
        }
    }

    /// 설정이 바뀌면 즉시 반영한다. 사용자가 슬라이더를 움직였는데 다음
    /// 재시작까지 그대로면 설정을 신뢰할 수 없게 된다.
    fn apply_profile(&mut self, profile: &Profile, now: Instant) {
        self.interval = Duration::from_millis(profile.interval_ms);
        self.limits = Limits {
            min_ms: profile.min_interval_ms,
            max_ms: profile.max_interval_ms,
        };
        self.adaptation_active = profile.adaptation_active();

        // 지금 모드에 맞는 길이로 다시 잡는다. 머무름 중에 슬라이더를 움직였다고
        // 순환 간격만큼만 머물게 되면, 화면의 남은 시간과 실제 마감이 어긋난다.
        let phase = match self.mode {
            Mode::Dwelling => self.dwell_timeout(),
            Mode::Confirm => CONFIRM_WINDOW,
            Mode::Scanning | Mode::Paused => self.interval,
        };
        self.enter(self.mode, phase, now);
    }

    /// 앱이 바뀌어 스캔 대상이 달라졌을 때.
    ///
    /// 커서를 첫 칸으로 되돌린다. 칸 수가 달라진 자리에 커서를 그대로 두면
    /// 사용자가 보고 있던 칸과 실제로 실행될 칸이 어긋난다.
    fn apply_cells(
        &mut self,
        cells: Vec<Cell>,
        preset_key: Option<String>,
        preset: Option<String>,
        now: Instant,
    ) {
        self.order = crate::action::scan_order(&cells);
        self.cells = cells;
        self.preset_key = preset_key;
        self.preset = preset;
        self.position = 0;
        self.restart_next = false;
        self.steps = 0;
        self.entered_at = now;
        self.pause_reason = None;
        self.enter(Mode::Scanning, self.interval, now);
    }

    /// 지금 커서가 있는 칸의 번호.
    fn cursor(&self) -> usize {
        self.order.get(self.position).copied().unwrap_or(0)
    }

    /// 모드와 그 모드가 지속되는 시간을 함께 바꾼다.
    ///
    /// `deadline`만 옮기고 `phase`를 놓치면 남은 시간 표시가 실제 마감과
    /// 어긋난다. 두 값을 항상 이 함수로만 움직여 어긋날 자리를 없앤다.
    fn enter(&mut self, mode: Mode, phase: Duration, now: Instant) {
        self.mode = mode;
        self.phase = phase;
        self.deadline = now + phase;
    }

    /// 이번 선택을 적응 로직에 넘기고, 간격이 바뀌면 그 내용을 돌려준다.
    fn observe(&mut self, now: Instant, undo: bool) -> Option<Adjustment> {
        if !self.adaptation_active {
            return None;
        }

        let sample = Sample {
            reaction_ms: now.duration_since(self.entered_at).as_millis() as u64,
            undo,
            missed: self.steps >= self.order.len() as u32,
        };

        let adjustment =
            self.adapter
                .observe(sample, self.interval.as_millis() as u64, &self.limits)?;
        self.interval = Duration::from_millis(adjustment.to_ms);
        Some(adjustment)
    }

    fn snapshot(&self, now: Instant) -> Snapshot {
        Snapshot {
            cursor: self.cursor(),
            cells: self
                .cells
                .iter()
                .map(|cell| CellView {
                    label: cell.label.clone(),
                    name: cell.name.clone(),
                    kind: cell.kind,
                })
                .collect(),
            preset: self.preset.clone(),
            mode: self.mode,
            interval_ms: self.interval.as_millis() as u64,
            phase_ms: self.phase.as_millis() as u64,
            // 마감이 이미 지났어도 음수가 되지 않는다. 창을 띄운 직후처럼
            // 틱과 스냅샷 요청이 엇갈리는 순간이 있다.
            remaining_ms: self.deadline.saturating_duration_since(now).as_millis() as u64,
        }
    }

    fn dwell_timeout(&self) -> Duration {
        self.interval * DWELL_FACTOR / DWELL_DIVISOR
    }

    fn step_cursor(&mut self, now: Instant) {
        // 선택한 직후에는 첫 이동 칸으로 돌아간다. 고르고 나서 다시 옮기는
        // 것이 가장 잦은 흐름인데, 이어서 순환하면 그 흐름이 한 바퀴를 돈다.
        self.position = if self.restart_next {
            self.restart_next = false;
            0
        } else {
            (self.position + 1) % self.order.len()
        };

        self.entered_at = now;

        // 놓침은 '원하는 칸을 지나쳐 한 바퀴를 더 기다린 것'이다(PRD 10.1).
        // 두 바퀴가 넘도록 누르지 않았다면 조작 중이 아니라 쉬고 있는 것이므로
        // 세지 않는다. 그러지 않으면 앱을 켜 둔 시간이 전부 놓침으로 집계된다.
        let idle_budget = self.interval * IDLE_CYCLES * self.order.len() as u32;
        if now.duration_since(self.last_input_at) > idle_budget {
            self.steps = 0;
            return;
        }

        self.steps = self.steps.saturating_add(1);
    }

    /// 시간 경과에 따른 전이. 상태가 바뀌었으면 새 스냅샷을 준다.
    fn advance(&mut self, now: Instant) -> Option<Snapshot> {
        if now < self.deadline {
            return None;
        }

        match self.mode {
            Mode::Scanning => {
                self.step_cursor(now);
                self.enter(Mode::Scanning, self.interval, now);
                Some(self.snapshot(now))
            }
            Mode::Dwelling => {
                // 머무름이 끝나면 다음 칸부터 순환을 재개한다.
                self.step_cursor(now);
                self.enter(Mode::Scanning, self.interval, now);
                Some(self.snapshot(now))
            }
            Mode::Confirm => {
                // 되돌리기 창이 지나면 머무름으로 내려간다.
                // 방금 Enter를 눌렀으니 한 번 더 누를 가능성이 높다.
                self.enter(Mode::Dwelling, self.dwell_timeout(), now);
                Some(self.snapshot(now))
            }
            Mode::Paused => {
                self.enter(Mode::Paused, self.interval, now);
                None
            }
        }
    }

    /// 짧게 누름. 실행할 부수효과와 간격 조정을 돌려준다.
    fn select(&mut self, now: Instant) -> Outcome {
        // 순환 중에 고른 것만 반응시간이 된다. 머무름 연타는 커서가 움직이지
        // 않아서 `entered_at`이 그대로이고, 그대로 재면 값이 계속 누적된다.
        let reaction_ms = (self.mode == Mode::Scanning)
            .then(|| now.duration_since(self.entered_at).as_millis() as u64);
        let steps = self.steps;
        let cycle = self.order.len();
        let cell = self.cells[self.cursor()].label.clone();
        let mode = self.mode.as_str();

        self.last_input_at = now;

        match self.mode {
            // 정지 중에는 어떤 동작도 실행하지 않는다.
            Mode::Paused => Outcome::default(),

            Mode::Confirm => {
                // 되돌리기가 나왔다는 건 직전 선택이 잘못됐다는 뜻이다.
                let adjustment = self.observe(now, true);

                // 되돌린 뒤에는 순환으로 돌아간다. 되돌리기를 머무름으로 두면
                // 한 번 더 누를 때 또 되돌아가 사용자가 위치를 잃는다.
                self.enter(Mode::Scanning, self.interval, now);
                self.steps = 0;

                Outcome {
                    command: Some(Command::Undo),
                    adjustment,
                    reaction_ms,
                    steps,
                    cycle,
                    cell,
                    mode,
                }
            }

            Mode::Scanning | Mode::Dwelling => {
                // 머무름 중의 연타는 '커서를 기다렸다가 누른' 것이 아니므로
                // 반응시간 표본이 아니다. 섞으면 실제보다 빠른 사용자로 오해한다.
                let adjustment = if self.mode == Mode::Scanning {
                    self.observe(now, false)
                } else {
                    None
                };
                self.steps = 0;

                let command = match self.cells[self.cursor()].action.clone() {
                    Action::Settings => {
                        self.enter(Mode::Dwelling, self.dwell_timeout(), now);
                        Command::OpenSettings
                    }
                    Action::Enter => {
                        self.restart_next = true;
                        self.enter(Mode::Confirm, CONFIRM_WINDOW, now);
                        Command::Emit(Action::Enter)
                    }
                    action => {
                        // 이동은 반대 방향 칸이 곧 되돌리기이므로 확인 창을 두지 않는다.
                        // 매 이동마다 3초를 기다리게 하면 연속 탐색이 불가능해진다.
                        self.restart_next = false;
                        self.enter(Mode::Dwelling, self.dwell_timeout(), now);
                        Command::Emit(action)
                    }
                };

                Outcome {
                    command: Some(command),
                    adjustment,
                    reaction_ms,
                    steps,
                    cycle,
                    cell,
                    mode,
                }
            }
        }
    }

    /// Halt because the active switch disappeared. Drops dwell/confirm deadlines
    /// so a later resume cannot fire a stale selection window.
    fn suspend_transport(&mut self, now: Instant) -> bool {
        if self.pause_reason == Some(PauseReason::User) {
            return false;
        }
        if self.mode == Mode::Paused && self.pause_reason == Some(PauseReason::Transport) {
            return false;
        }
        self.pause_reason = Some(PauseReason::Transport);
        if self.mode == Mode::Paused {
            return false;
        }
        self.enter(Mode::Paused, self.interval, now);
        true
    }

    /// Resume only a transport halt, at the same cursor with a fresh interval.
    fn resume_transport(&mut self, now: Instant) -> bool {
        if self.pause_reason != Some(PauseReason::Transport) {
            return false;
        }
        self.pause_reason = None;
        self.enter(Mode::Scanning, self.interval, now);
        true
    }

    /// 길게 누름. 정지와 해제를 오간다.
    fn toggle_pause(&mut self, now: Instant) {
        self.last_input_at = now;
        if self.mode == Mode::Paused {
            self.pause_reason = None;
            self.enter(Mode::Scanning, self.interval, now);
        } else {
            self.pause_reason = Some(PauseReason::User);
            self.enter(Mode::Paused, self.interval, now);
        }
    }
}

/// 입력에 대해 어떤 소리를 낼지 정한다.
///
/// 상태 전이와 분리해 두면 소리 규칙만 따로 테스트할 수 있다.
fn cue_for(gesture: Gesture, command: Option<&Command>, mode: Mode) -> Option<Cue> {
    if gesture == Gesture::Long {
        // 정지와 해제는 반드시 구분되어야 한다. 해제를 조용히 넘기면
        // 사용자는 다시 움직이기 시작했는지 화면을 봐야만 알 수 있다.
        return Some(if mode == Mode::Paused {
            Cue::Pause
        } else {
            Cue::Select
        });
    }

    match command? {
        Command::Undo => Some(Cue::Undo),
        // 되돌리기 창에 들어간 것도 사용자가 알아야 하는 상태 변화다.
        _ if mode == Mode::Confirm => Some(Cue::Undo),
        _ => Some(Cue::Select),
    }
}

#[derive(Clone)]
pub struct Scanner {
    state: Arc<Mutex<State>>,
    /// 플랫폼이 해 주는 일. 무엇을 할지는 여기서 정하고, 어떻게 할지는 모른다.
    host: Arc<dyn Host>,
    profile: Arc<Mutex<Profile>>,
    journal: Journal,
    running: Arc<AtomicBool>,
}

impl Scanner {
    pub fn new(profile: Arc<Mutex<Profile>>, host: Arc<dyn Host>, journal: Journal) -> Self {
        let state = {
            let profile = profile.lock().expect("프로필 잠금 실패");
            State::new(&profile, Instant::now())
        };

        Self {
            state: Arc::new(Mutex::new(state)),
            host,
            profile,
            journal,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 설정이 저장되면 호출한다. 소리 on/off까지 함께 반영한다.
    pub fn apply_profile(&self) {
        let Ok(profile) = self.profile.lock() else {
            return;
        };
        self.host.set_sound(profile.sound);
        if let Ok(mut state) = self.state.lock() {
            state.apply_profile(&profile, Instant::now());
        }
    }

    /// 포그라운드 앱에 맞는 칸으로 갈아 끼운다.
    ///
    /// 바뀐 것이 없으면 아무것도 하지 않는다. 이 함수는 300ms마다 불리므로
    /// 매번 상태를 갈아엎으면 커서가 제자리를 맴돌아 스캔이 진행되지 않는다.
    pub fn apply_preset(&self, bundle_id: Option<&str>) {
        let enabled = self
            .profile
            .lock()
            .map(|profile| profile.app_buttons)
            .unwrap_or(false);

        let preset = if enabled {
            bundle_id.and_then(crate::preset::for_bundle)
        } else {
            None
        };
        let name = preset.map(|preset| preset.name.to_string());

        let snapshot = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.preset_key == name {
                return;
            }
            let now = Instant::now();
            state.apply_cells(
                crate::preset::cells_for(preset),
                name.clone(),
                name.clone(),
                now,
            );
            state.snapshot(now)
        };

        let message = match &name {
            Some(name) => format!("{name} — 버튼 {}개 추가", snapshot.cells.len() - 4),
            None => "앱별 버튼 없음".to_string(),
        };

        if log_enabled() {
            eprintln!("[preset] {message} (칸 {})", snapshot.cells.len());
        }

        // 칸 수가 달라졌으니 화면도 맞춘다.
        let extras = snapshot
            .cells
            .iter()
            .filter(|cell| cell.kind == Kind::Extra)
            .count();
        self.host.fit_cells(extras);

        self.journal.record(Event::Preset {
            preset: name.clone(),
            cells: snapshot.cells.len(),
        });

        // 바뀌었다는 사실을 두 감각으로 알린다(PRD 원칙 4).
        self.host.cue(Cue::Select);
        self.host.publish(Notice::State(Box::new(snapshot)));
        self.host.publish(Notice::Preset { message });
    }

    /// 플랫폼 registry가 고른 동적 프리셋을 적용한다.
    ///
    /// registry 조회와 검증은 플랫폼의 책임이고, 스캔 순서 교체와 알림은 코어가
    /// 맡는다. `key`에는 내용 버전을 포함해야 같은 이름의 프리셋이 갱신되어도
    /// 즉시 갈아 끼울 수 있다.
    pub fn apply_cells(
        &self,
        key: Option<String>,
        registry_id: Option<String>,
        name: Option<String>,
        cells: Vec<Cell>,
    ) {
        let (snapshot, first_recognition) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.preset_key == key {
                return;
            }
            let first_recognition = registry_id
                .as_ref()
                .is_some_and(|id| state.recognized_presets.insert(id.clone()));
            let now = Instant::now();
            state.apply_cells(cells, key, name.clone(), now);
            (state.snapshot(now), first_recognition)
        };

        let extras = snapshot
            .cells
            .iter()
            .filter(|cell| cell.kind == Kind::Extra)
            .count();
        let message = match name.as_deref() {
            Some(name) if first_recognition => {
                format!("{name} 프로필 인식 완료 · 버튼 {extras}개 추가")
            }
            Some(name) => format!("{name} — 버튼 {extras}개 추가"),
            None => "앱별 버튼 없음".to_string(),
        };

        if log_enabled() {
            eprintln!("[preset] {message} (칸 {})", snapshot.cells.len());
        }

        self.host.fit_cells(extras);
        self.journal.record(Event::Preset {
            preset: name,
            cells: snapshot.cells.len(),
        });
        self.host.cue(Cue::Select);
        self.host.publish(Notice::State(Box::new(snapshot)));
        self.host.publish(Notice::Preset { message });
    }

    /// 스위치가 뽑혔다. 눌림은 판정하지 않고 스캔을 정지로 내린다(PRD F10).
    ///
    /// 커서만 계속 도는 상태로 두면 사용자는 눌러도 아무 일이 없는 이유를 알 수
    /// 없다. 사용자가 길게 눌러 멈춘 정지는 건드리지 않는다. 전송이 돌아와
    /// `resume_transport`를 부르면 같은 커서에서 순환만 재개한다.
    pub fn switch_lost(&self) {
        self.suspend_transport();
    }

    /// Halt scanning because the active input source disappeared.
    ///
    /// Dwell/confirm deadlines are dropped so recovery cannot fire a stale
    /// selection. An explicit long-press pause is left untouched.
    pub fn suspend_transport(&self) {
        let snapshot = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let now = Instant::now();
            if !state.suspend_transport(now) {
                return;
            }
            state.snapshot(now)
        };

        if log_enabled() {
            eprintln!("[scan] 스위치 연결이 끊겨 정지합니다");
        }
        self.host.cue(Cue::Pause);
        self.host.publish(Notice::State(Box::new(snapshot)));
    }

    /// Resume only a transport-caused halt at the same cursor with a full interval.
    pub fn resume_transport(&self) {
        let snapshot = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let now = Instant::now();
            if !state.resume_transport(now) {
                return;
            }
            state.snapshot(now)
        };

        if log_enabled() {
            eprintln!("[scan] 스위치 연결이 돌아와 순환을 재개합니다");
        }
        self.host.publish(Notice::State(Box::new(snapshot)));
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        let now = Instant::now();
        self.state.lock().ok().map(|state| state.snapshot(now))
    }

    /// 상태 확인 루프를 띄운다.
    ///
    /// 모드마다 마감이 다르므로 고정 간격으로 자는 대신 짧은 주기로 마감을
    /// 확인한다. 입력으로 마감이 바뀌어도 한 틱 안에 반영된다.
    pub fn start(&self) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let running = Arc::clone(&self.running);
        let state = Arc::clone(&self.state);
        let host = Arc::clone(&self.host);
        let journal = self.journal.clone();

        thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                thread::sleep(TICK);
                if !running.load(Ordering::Acquire) {
                    return;
                }

                let changed = {
                    let Ok(mut state) = state.lock() else {
                        return;
                    };
                    state.advance(Instant::now())
                };

                if let Some(snapshot) = changed {
                    // 커서가 움직였을 때만 틱을 낸다. 되돌리기 창이 만료돼
                    // 머무름으로 내려가는 것은 화면 변화로 충분하다.
                    if snapshot.mode == Mode::Scanning {
                        host.cue(Cue::Tick);
                    }

                    journal.record(Event::Cursor {
                        cursor: snapshot.cursor,
                        cell: snapshot
                            .cells
                            .get(snapshot.cursor)
                            .map(|cell| cell.label.clone())
                            .unwrap_or_default(),
                        mode: snapshot.mode.as_str(),
                        interval_ms: snapshot.interval_ms,
                    });

                    host.publish(Notice::State(Box::new(snapshot)));
                }
            }
        });
    }

    /// Stop the timer worker so a platform service teardown releases the scanner.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    /// 판정된 제스처를 상태기계에 넣는다.
    pub fn handle(&self, judgement: Judgement) {
        let Judgement { gesture, held } = judgement;

        let (outcome, snapshot) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let now = Instant::now();
            let outcome = match gesture {
                Gesture::Short => state.select(now),
                Gesture::Long => {
                    state.toggle_pause(now);
                    Outcome::default()
                }
            };
            (outcome, state.snapshot(now))
        };

        if log_enabled() {
            eprintln!(
                "[scan] {gesture:?} -> {:?} (mode={:?}, cursor={}, interval={}ms)",
                outcome.command, snapshot.mode, snapshot.cursor, snapshot.interval_ms
            );
        }

        // 스냅샷은 곧 프론트로 넘어가며 소유권을 잃는다. 기록에 쓸 값은
        // 그 전에 챙겨 둔다.
        let mode = snapshot.mode;

        // 화면과 소리를 먼저 바꾸고 키를 보낸다. 주입이 늦어져도 사용자는
        // 자기 입력이 받아들여졌다는 것을 즉시 알 수 있어야 한다.
        if let Some(cue) = cue_for(gesture, outcome.command.as_ref(), snapshot.mode) {
            self.host.cue(cue);
        }
        self.host.publish(Notice::State(Box::new(snapshot)));

        if let Some(adjustment) = outcome.adjustment {
            self.announce(adjustment);
        }

        if gesture == Gesture::Long {
            self.journal.record(Event::Pause {
                paused: mode == Mode::Paused,
            });
        }

        self.journal.record(Event::Input {
            gesture: match gesture {
                Gesture::Short => "short",
                Gesture::Long => "long",
            },
            held_ms: held.as_millis() as u64,
        });

        if let Some(command) = outcome.command {
            let mapping = self
                .profile
                .lock()
                .map(|profile| profile.undo_mapping)
                .unwrap_or_default();

            let undo = command == Command::Undo;
            let name = command_name(&command);
            let failure = execute(self.host.as_ref(), command, mapping);

            self.journal.record(if undo {
                Event::Undo {
                    mapping: format!("{mapping:?}").to_lowercase(),
                    ok: failure.is_none(),
                }
            } else {
                Event::Action {
                    cell: outcome.cell,
                    action: name,
                    reaction_ms: outcome.reaction_ms,
                    mode: outcome.mode,
                    steps: outcome.steps,
                    cycle: outcome.cycle,
                    ok: failure.is_none(),
                    error: failure,
                }
            });
        }
    }

    /// 간격이 왜 바뀌었는지 알린다.
    ///
    /// 이유를 알 수 없는 변화는 사용자의 통제감을 무너뜨린다(PRD 원칙 2).
    /// 조정된 값은 프로필에도 반영해 다음 실행으로 이어진다.
    fn announce(&self, adjustment: Adjustment) {
        let description = adjustment
            .reason
            .describe(adjustment.from_ms, adjustment.to_ms);

        if let Ok(mut profile) = self.profile.lock() {
            profile.interval_ms = adjustment.to_ms;
            // 바로 적어 둔다. 종료 시에만 저장하면 강제 종료·정전에서
            // 사용자가 익숙해진 속도를 잃는다. 조정은 자주 일어나지 않고
            // 파일도 작아서 그때마다 쓰는 편이 안전하다.
            self.host.save_profile(&profile);
        }

        if log_enabled() {
            eprintln!("[scan] 간격 조정: {description}");
        }

        self.journal.record(Event::Interval {
            from_ms: adjustment.from_ms,
            to_ms: adjustment.to_ms,
            reason: description.clone(),
        });

        self.host.publish(Notice::Interval {
            from_ms: adjustment.from_ms,
            to_ms: adjustment.to_ms,
            reason: description,
        });
    }
}

/// `HANBEON_LOG=1`이면 상태 전이와 실행 결과를 stderr로 찍는다.
///
/// 정량 검증(PRD 10절)에는 어차피 이벤트 기록이 필요하다. 지금은 진단용
/// 최소 형태이고, M4에서 파일 로그로 확장한다.
fn log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("HANBEON_LOG").is_ok())
}

/// 검증할 때 타이밍을 넉넉히 잡기 위한 개발용 통로.
/// 저장된 프로필을 건드리지 않고 시작 간격만 바꿔 끼운다.
pub fn interval_override() -> Option<u64> {
    std::env::var("HANBEON_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

/// 기록에 남길 동작 이름.
fn command_name(command: &Command) -> String {
    match command {
        Command::Emit(Action::Next) => "next".into(),
        Command::Emit(Action::Prev) => "prev".into(),
        Command::Emit(Action::Enter) => "enter".into(),
        Command::Emit(Action::Settings) | Command::OpenSettings => "settings".into(),
        Command::Emit(Action::Shortcut(shortcut)) => format!("shortcut:{:?}", shortcut.key),
        Command::Undo => "undo".into(),
    }
}

/// 실행하고, 실패했으면 그 사유를 돌려준다.
fn execute(host: &dyn Host, command: Command, mapping: UndoMapping) -> Option<String> {
    let result = match command {
        Command::Emit(action) => host.inject(action),
        Command::Undo => host.undo(mapping),
        Command::OpenSettings => host.open_settings(),
    };

    match result {
        Ok(()) => None,
        Err(error) => {
            if log_enabled() {
                eprintln!("[scan] 실행 실패: {}", error.message);
            }
            let message = error.message.clone();
            host.publish(Notice::Error {
                message: error.message,
                needs_permission: error.needs_permission,
            });
            Some(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_millis(1000);

    /// 상태기계만 보는 테스트. 적응은 꺼서 간격이 흔들리지 않게 한다.
    fn fixed_profile() -> Profile {
        Profile {
            interval_ms: 1000,
            min_interval_ms: 1000,
            max_interval_ms: 1000,
            adaptive: false,
            ..Default::default()
        }
    }

    fn state() -> (State, Instant) {
        let now = Instant::now();
        (State::new(&fixed_profile(), now), now)
    }

    #[test]
    fn 순환_중_선택은_반응시간을_남긴다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);

        // 커서가 칸에 들어오고 300ms 뒤에 누른다.
        let outcome = state.select(now + Duration::from_millis(300));

        assert_eq!(outcome.mode, "scanning");
        assert_eq!(outcome.reaction_ms, Some(300));
    }

    #[test]
    fn 머무름_연타는_반응시간을_남기지_않는다() {
        // 머무름 중에는 커서가 움직이지 않아 `entered_at`이 그대로다. 그대로 재면
        // 값이 계속 누적되어 반응속도 지표가 통째로 부풀려진다.
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);

        state.select(now);
        assert_eq!(state.mode, Mode::Dwelling);

        let first = state.select(now + Duration::from_millis(400));
        let second = state.select(now + Duration::from_millis(800));

        assert_eq!(first.mode, "dwelling");
        assert_eq!(first.reaction_ms, None);
        assert_eq!(second.reaction_ms, None);
    }

    #[test]
    fn 한_바퀴를_지나치면_놓침으로_남는다() {
        let (mut state, mut now) = state();

        // 누르지 않고 한 바퀴를 돌되, 쉬고 있다고 볼 만큼은 아니다.
        for _ in 0..state.order.len() {
            now += INTERVAL;
            state.advance(now);
        }

        let outcome = state.select(now);
        assert!(outcome.steps >= outcome.cycle as u32);
    }

    #[test]
    fn 오래_쉬면_지나온_자리를_놓침으로_세지_않는다() {
        // 앱을 켜 두고 조작하지 않은 시간이 전부 놓침으로 집계되던 문제.
        let (mut state, mut now) = state();

        let idle = INTERVAL * IDLE_CYCLES * state.order.len() as u32;
        let long_idle = now + idle + INTERVAL * 3;
        while now < long_idle {
            now += INTERVAL;
            state.advance(now);
        }

        let outcome = state.select(now);
        assert!(
            (outcome.steps as usize) < outcome.cycle,
            "쉬고 있던 시간이 놓침으로 잡혔다 (steps={}, cycle={})",
            outcome.steps,
            outcome.cycle
        );
    }

    #[test]
    fn 정지_중에는_지나온_자리가_늘지_않는다() {
        let (mut state, mut now) = state();
        state.toggle_pause(now);
        assert_eq!(state.mode, Mode::Paused);

        let before = state.steps;
        for _ in 0..10 {
            now += INTERVAL;
            state.advance(now);
        }

        assert_eq!(state.steps, before);
    }

    /// 커서를 원하는 칸으로 옮긴다.
    fn move_to(state: &mut State, target: Action, start: Instant) -> Instant {
        let mut now = start;
        for _ in 0..state.order.len() {
            if state.cells[state.cursor()].action == target {
                return now;
            }
            now += INTERVAL;
            state.advance(now);
        }
        panic!("커서를 {target:?}로 옮기지 못했습니다");
    }

    #[test]
    fn 커서는_스캔_순서를_따른다() {
        let (mut state, mut now) = state();
        // 이동 칸마다 뒤에 선택이 끼어든다. 옮기고 나서 가장 자주 하는 일이다.
        let order = [
            Action::Next,
            Action::Enter,
            Action::Prev,
            Action::Enter,
            Action::Settings,
            Action::Next,
        ];

        for expected in order {
            assert_eq!(state.cells[state.cursor()].action, expected);
            now += INTERVAL;
            state.advance(now);
        }
    }

    #[test]
    fn 초기_상태는_첫_칸에서_순환_중이다() {
        let (state, now) = state();
        let snapshot = state.snapshot(now);
        assert_eq!(snapshot.cursor, 0);
        assert_eq!(snapshot.mode, Mode::Scanning);
    }

    // --- 남은 시간 표시 ---

    #[test]
    fn 남은_시간은_마감까지_줄어든다() {
        let (state, now) = state();

        assert_eq!(state.snapshot(now).remaining_ms, 1000);
        assert_eq!(
            state
                .snapshot(now + Duration::from_millis(400))
                .remaining_ms,
            600
        );
    }

    #[test]
    fn 마감이_지나도_남은_시간은_음수가_되지_않는다() {
        let (state, now) = state();
        assert_eq!(
            state
                .snapshot(now + Duration::from_millis(1500))
                .remaining_ms,
            0
        );
    }

    #[test]
    fn 모드마다_표시_길이가_다르다() {
        let (mut state, now) = state();

        // 순환은 주사 간격 그대로.
        assert_eq!(state.snapshot(now).phase_ms, 1000);

        // 머무름은 주사 간격의 1.5배.
        state.select(now);
        assert_eq!(state.mode, Mode::Dwelling);
        assert_eq!(state.snapshot(now).phase_ms, 1500);

        // 되돌리기 창은 주사 간격과 무관하게 3초.
        let now = move_to(&mut state, Action::Enter, now);
        state.select(now);
        assert_eq!(state.mode, Mode::Confirm);
        assert_eq!(state.snapshot(now).phase_ms, 3000);
    }

    #[test]
    fn 설정이_바뀌면_표시_길이도_지금_모드에_맞춰_따라온다() {
        let (mut state, now) = state();

        state.select(now);
        assert_eq!(state.mode, Mode::Dwelling);

        let profile = Profile {
            interval_ms: 2000,
            min_interval_ms: 800,
            max_interval_ms: 4000,
            ..Default::default()
        };
        state.apply_profile(&profile, now);

        // 머무름 중이었으므로 새 간격의 1.5배로 다시 잡혀야 한다.
        assert_eq!(state.snapshot(now).phase_ms, 3000);
        assert_eq!(state.deadline, now + Duration::from_millis(3000));
    }

    #[test]
    fn 마감_전에는_아무_일도_없다() {
        let (mut state, now) = state();
        assert!(state.advance(now + Duration::from_millis(999)).is_none());
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn 순환은_주사_간격마다_한_자리씩_움직인다() {
        let (mut state, now) = state();

        // > → Enter → < 순으로 간다. 이동 칸 뒤에는 언제나 선택이 온다.
        assert_eq!(state.cells[state.cursor()].action, Action::Next);
        assert!(state.advance(now + INTERVAL).is_some());
        assert_eq!(state.cells[state.cursor()].action, Action::Enter);
        assert!(state.advance(now + INTERVAL * 2).is_some());
        assert_eq!(state.cells[state.cursor()].action, Action::Prev);
    }

    #[test]
    fn 이동을_선택하면_그_칸에_머문다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);

        assert_eq!(state.select(now).command, Some(Command::Emit(Action::Next)));
        assert_eq!(state.mode, Mode::Dwelling);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn 머무는_동안_다시_누르면_같은_동작을_반복한다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);
        state.select(now);

        // 누름 1회 = 이동 1칸. 순환을 기다리지 않는다.
        let now = now + Duration::from_millis(200);
        assert_eq!(state.select(now).command, Some(Command::Emit(Action::Next)));
        assert_eq!(state.mode, Mode::Dwelling);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn 머무름이_끝나면_다음_칸부터_순환을_재개한다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);
        state.select(now);

        let dwell = state.dwell_timeout();
        assert!(
            state
                .advance(now + dwell - Duration::from_millis(1))
                .is_none()
        );
        assert!(state.advance(now + dwell).is_some());
        assert_eq!(state.mode, Mode::Scanning);
        // 이동 칸에 머문 뒤에는 선택이 온다. 옮기고 나서 가장 자주 하는 일이다.
        assert_eq!(state.cells[state.cursor()].action, Action::Enter);
    }

    #[test]
    fn 지나쳐도_이전으로_돌아갈_수_있다() {
        let (mut state, now) = state();

        // 다음으로 두 번 이동했다가 지나쳤다고 하자.
        let now = move_to(&mut state, Action::Next, now);
        state.select(now);
        let now = now + Duration::from_millis(200);
        state.select(now);

        // 머무름이 끝나면 선택이 한 자리 오고, 그 다음이 '이전으로'다.
        let mut now = now + state.dwell_timeout();
        state.advance(now);
        assert_eq!(state.cells[state.cursor()].action, Action::Enter);

        now += INTERVAL;
        state.advance(now);
        assert_eq!(state.cells[state.cursor()].action, Action::Prev);

        // 한 번 누르면 되돌아간다. 한 바퀴를 기다리지 않는다.
        assert_eq!(state.select(now).command, Some(Command::Emit(Action::Prev)));
    }

    #[test]
    fn 선택_직후에는_되돌리기_창이_열린다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Enter, now);

        assert_eq!(
            state.select(now).command,
            Some(Command::Emit(Action::Enter))
        );
        assert_eq!(state.mode, Mode::Confirm);

        assert_eq!(
            state.select(now + Duration::from_secs(1)).command,
            Some(Command::Undo)
        );
        assert_eq!(state.mode, Mode::Scanning);
    }

    #[test]
    fn 선택한_뒤에는_첫_이동_칸부터_다시_시작한다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Enter, now);

        state.select(now);
        assert_eq!(state.mode, Mode::Confirm);

        // 되돌리기 창 → 머무름 → 그 다음 자리.
        let now = now + CONFIRM_WINDOW;
        state.advance(now);
        let now = now + state.dwell_timeout();
        state.advance(now);

        // 이어서 순환하지 않고 처음으로 돌아온다. 고르고 나서 다시 옮기는
        // 것이 가장 잦은 흐름이라, 그 흐름이 한 바퀴를 돌지 않게 한다.
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.cells[state.cursor()].action, Action::Next);
    }

    #[test]
    fn 이동한_뒤에는_처음으로_돌아가지_않는다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);

        state.select(now);
        let now = now + state.dwell_timeout();
        state.advance(now);

        // 이동은 이어서 순환한다. 다음 자리는 선택이다.
        assert_eq!(state.cells[state.cursor()].action, Action::Enter);
    }

    #[test]
    fn 되돌리기_창은_삼초_뒤_머무름으로_내려간다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Enter, now);
        state.select(now);

        assert!(
            state
                .advance(now + CONFIRM_WINDOW - Duration::from_millis(1))
                .is_none()
        );
        assert!(state.advance(now + CONFIRM_WINDOW).is_some());
        assert_eq!(state.mode, Mode::Dwelling);

        // 이제 누르면 되돌리기가 아니라 Enter가 한 번 더 나간다.
        let now = now + CONFIRM_WINDOW;
        assert_eq!(
            state.select(now).command,
            Some(Command::Emit(Action::Enter))
        );
    }

    #[test]
    fn 이동에는_되돌리기_창을_두지_않는다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Next, now);
        state.select(now);
        // 매 이동마다 3초를 기다리면 연속 탐색이 불가능해진다.
        assert_eq!(state.mode, Mode::Dwelling);
    }

    #[test]
    fn 설정_칸은_설정_창을_연다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Settings, now);
        assert_eq!(state.select(now).command, Some(Command::OpenSettings));
        assert_eq!(state.mode, Mode::Dwelling);
    }

    #[test]
    fn 길게_누르면_정지하고_다시_누르면_풀린다() {
        let (mut state, now) = state();

        state.toggle_pause(now);
        assert_eq!(state.mode, Mode::Paused);

        // 정지 중에는 순환도 실행도 없다.
        assert!(state.advance(now + INTERVAL).is_none());
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.select(now + INTERVAL).command, None);

        state.toggle_pause(now + INTERVAL);
        assert_eq!(state.mode, Mode::Scanning);
    }

    #[test]
    fn 정지와_해제는_서로_다른_소리를_낸다() {
        assert_eq!(cue_for(Gesture::Long, None, Mode::Paused), Some(Cue::Pause));
        assert_eq!(
            cue_for(Gesture::Long, None, Mode::Scanning),
            Some(Cue::Select)
        );
    }

    #[test]
    fn 되돌리기_창_진입과_되돌리기는_같은_소리를_낸다() {
        assert_eq!(
            cue_for(
                Gesture::Short,
                Some(&Command::Emit(Action::Enter)),
                Mode::Confirm
            ),
            Some(Cue::Undo)
        );
        assert_eq!(
            cue_for(Gesture::Short, Some(&Command::Undo), Mode::Scanning),
            Some(Cue::Undo)
        );
    }

    #[test]
    fn 정지_중_짧게_누름은_소리를_내지_않는다() {
        assert_eq!(cue_for(Gesture::Short, None, Mode::Paused), None);
    }

    #[test]
    fn 되돌리기_창에서도_길게_누르면_정지한다() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Enter, now);
        state.select(now);
        assert_eq!(state.mode, Mode::Confirm);

        state.toggle_pause(now);
        assert_eq!(state.mode, Mode::Paused);
    }

    // --- 적응 로직과의 연결 ---

    fn adaptive_profile() -> Profile {
        Profile {
            interval_ms: 2000,
            min_interval_ms: 800,
            max_interval_ms: 4000,
            adaptive: true,
            manual_lock: false,
            ..Default::default()
        }
    }

    /// 커서가 칸에 들어온 뒤 `reaction` 만큼 기다렸다가 누르기를 반복한다.
    /// 머무름이 풀리면 커서는 다음 칸으로 넘어가므로 '다음으로 → 이전으로 → 선택'
    /// 순서로 눌리게 된다.
    fn press_repeatedly(state: &mut State, mut now: Instant, reaction: Duration, times: usize) {
        for _ in 0..times {
            // 반응시간 표본은 순환 중 이동 칸에서만 모인다. 선택 칸에서 누르면
            // 되돌리기 창이 열리고, 그 다음 누름은 표본이 아니라 '실수'가 된다.
            while state.mode != Mode::Scanning || state.cells[state.cursor()].kind != Kind::Move {
                now += INTERVAL;
                state.advance(now);
            }

            now += reaction;
            state.select(now);

            now += state.dwell_timeout();
            state.advance(now);
        }
    }

    #[test]
    fn 반응이_빠르면_간격이_줄어든다() {
        let now = Instant::now();
        let mut state = State::new(&adaptive_profile(), now);

        press_repeatedly(&mut state, now, Duration::from_millis(300), 3);
        assert!(state.interval < Duration::from_millis(2000));
    }

    #[test]
    fn 수동_고정이면_간격이_그대로다() {
        let now = Instant::now();
        let profile = Profile {
            manual_lock: true,
            ..adaptive_profile()
        };
        let mut state = State::new(&profile, now);

        press_repeatedly(&mut state, now, Duration::from_millis(300), 3);
        assert_eq!(state.interval, Duration::from_millis(2000));
    }

    #[test]
    fn 머무름_중의_연타는_반응시간_표본이_아니다() {
        let now = Instant::now();
        let mut state = State::new(&adaptive_profile(), now);

        // 순환 중 한 번 누른 뒤, 머무는 동안 아주 빠르게 여러 번 누른다.
        let now = now + Duration::from_millis(1500);
        state.select(now);
        let mut now = now;
        for _ in 0..10 {
            now += Duration::from_millis(10);
            assert!(state.select(now).adjustment.is_none());
        }
    }

    #[test]
    fn 설정을_바꾸면_즉시_반영된다() {
        let (mut state, now) = state();
        let profile = Profile {
            interval_ms: 2500,
            min_interval_ms: 800,
            max_interval_ms: 4000,
            ..Default::default()
        };

        state.apply_profile(&profile, now);
        assert_eq!(state.interval, Duration::from_millis(2500));
        assert_eq!(state.deadline, now + Duration::from_millis(2500));
    }

    #[test]
    fn transport_loss_while_released_suspends_and_resumes_same_cursor() {
        let (mut state, mut now) = state();
        now += INTERVAL;
        state.advance(now);
        let cursor = state.cursor();
        let remaining_before = state
            .snapshot(now + Duration::from_millis(400))
            .remaining_ms;

        assert!(state.suspend_transport(now + Duration::from_millis(400)));
        assert_eq!(state.mode, Mode::Paused);
        assert_eq!(state.cursor(), cursor);
        assert!(state.advance(now + INTERVAL * 3).is_none());

        let resumed_at = now + INTERVAL * 3;
        assert!(state.resume_transport(resumed_at));
        assert_eq!(state.mode, Mode::Scanning);
        assert_eq!(state.cursor(), cursor);
        assert_eq!(state.snapshot(resumed_at).remaining_ms, 1000);
        assert!(remaining_before < 1000);
        assert!(
            state
                .advance(resumed_at + INTERVAL - Duration::from_millis(1))
                .is_none()
        );
        assert!(state.advance(resumed_at + INTERVAL).is_some());
    }

    #[test]
    fn transport_loss_does_not_resume_manual_pause() {
        let (mut state, now) = state();
        state.toggle_pause(now);
        assert_eq!(state.mode, Mode::Paused);

        assert!(!state.suspend_transport(now + Duration::from_millis(10)));
        assert!(!state.resume_transport(now + Duration::from_millis(20)));
        assert_eq!(state.mode, Mode::Paused);
    }

    #[test]
    fn transport_loss_drops_stale_dwell_and_confirm_deadlines() {
        let (mut state, now) = state();
        let now = move_to(&mut state, Action::Enter, now);
        state.select(now);
        assert_eq!(state.mode, Mode::Confirm);
        let cursor = state.cursor();

        assert!(state.suspend_transport(now + Duration::from_millis(10)));
        assert_eq!(state.mode, Mode::Paused);

        let later = now + CONFIRM_WINDOW + Duration::from_millis(50);
        assert!(state.advance(later).is_none());
        assert!(state.resume_transport(later));
        assert_eq!(state.mode, Mode::Scanning);
        assert_eq!(state.cursor(), cursor);
        assert_eq!(state.snapshot(later).remaining_ms, 1000);
    }
}
