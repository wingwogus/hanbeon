//! 실증용 이벤트 기록.
//!
//! PRD 10절의 정량 지표는 실측으로만 주장할 수 있는데, 지금까지는 잴 수단이
//! 없었다. 무엇이 언제 일어났는지를 파일로 남겨야 성공률·오선택·되돌리기를
//! 나중에 셀 수 있다.
//!
//! **쓰기는 별도 스레드에서 한다.** 커서 이동은 주사 간격마다 일어나는데,
//! 그 자리에서 파일에 쓰면 디스크가 느릴 때 주사 간격이 흔들린다. 간격 오차
//! 예산은 ±30ms뿐이다(PRD 9절).
//!
//! **기록은 이 기기 밖으로 나가지 않는다.** 실증 참여자의 동의 없이 외부로
//! 전송하지 않는다는 것이 PRD 10절의 약속이고, 그래서 네트워크 코드를 두지
//! 않는다.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;

use chrono::{DateTime, Local};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Manager};

/// 한 줄에 하나씩 남기는 사건.
///
/// 값은 모두 문자열·숫자로만 둔다. 코어의 타입을 그대로 넣으면 타입이 바뀔
/// 때마다 지난 기록을 읽을 수 없게 된다.
#[derive(Clone, Debug, Serialize)]
// `rename_all`은 변형 이름에만 걸린다. 항목 이름까지 camelCase로 맞추려면
// `rename_all_fields`가 따로 필요하다.
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    /// 앱이 뜨고 졌다.
    Session {
        phase: &'static str,
        version: String,
    },
    /// 스위치 키를 잡았거나 놓쳤다.
    Switch { state: &'static str, key: String },
    /// 커서가 다른 칸으로 옮겨 갔다.
    Cursor {
        cursor: usize,
        cell: String,
        mode: &'static str,
        interval_ms: u64,
    },
    /// 스위치 눌림을 짧게/길게로 판정했다.
    Input { gesture: &'static str, held_ms: u64 },
    /// 칸의 동작을 실행했다.
    ///
    /// `reaction_ms`는 커서가 그 칸에 들어온 뒤 누르기까지 걸린 시간이고,
    /// `steps`는 지난 선택 이후 커서가 지나온 자리 수다. 한 바퀴를 넘겼다면
    /// 원하는 칸을 지나쳐 다시 기다린 것이다.
    Action {
        cell: String,
        action: String,
        reaction_ms: u64,
        steps: u32,
        /// 그때 순환에 있던 자리 수. `steps`가 이 값 이상이면 원하는 칸을
        /// 지나쳐 한 바퀴를 더 기다린 것이다. 자리 수는 앱에 따라 달라지므로
        /// 함께 남기지 않으면 나중에 셀 수 없다.
        cycle: usize,
        ok: bool,
        error: Option<String>,
    },
    /// 길게 눌러 정지하거나 풀었다.
    ///
    /// 이게 없으면 정지 구간이 그냥 '기록이 끊긴 자리'로 보인다. 분석할 때
    /// 멈춘 것과 앱이 죽은 것과 자리를 비운 것을 가릴 수 없다.
    Pause { paused: bool },
    /// 되돌리기를 실행했다.
    Undo { mapping: String, ok: bool },
    /// 적응 로직이 주사 간격을 바꿨다.
    Interval {
        from_ms: u64,
        to_ms: u64,
        reason: String,
    },
    /// 앱이 바뀌어 스캔 대상이 달라졌다.
    Preset {
        preset: Option<String>,
        cells: usize,
    },
}

/// 기록을 받아 파일에 적는 통로.
///
/// 꺼져 있거나 파일을 열지 못하면 아무 일도 하지 않는다. 기록을 남기지
/// 못한다고 해서 앱이 멈추면 사용자는 조작 수단을 잃는다.
#[derive(Clone)]
pub struct Journal {
    tx: Option<Sender<Event>>,
}

impl Journal {
    /// 아무것도 적지 않는 기록기.
    pub fn off() -> Self {
        Self { tx: None }
    }

    /// 로그 폴더에 오늘 날짜 파일을 열고 쓰기 스레드를 띄운다.
    pub fn open(app: &AppHandle) -> Self {
        let Some(path) = today_path(app) else {
            eprintln!("기록 폴더를 찾지 못해 이벤트를 남기지 않습니다.");
            return Self::off();
        };

        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("기록 파일을 열지 못해 이벤트를 남기지 않습니다. ({error})");
                return Self::off();
            }
        };

        let (tx, rx) = mpsc::channel::<Event>();

        thread::spawn(move || {
            let mut writer = BufWriter::new(file);

            for event in rx {
                if let Err(error) = write_line(&mut writer, &event) {
                    eprintln!("이벤트를 남기지 못했습니다. ({error})");
                    break;
                }
            }

            // 채널이 닫히면 앱이 끝난 것이다. 남은 것을 마저 적는다.
            let _ = writer.flush();
        });

        Self { tx: Some(tx) }
    }

    /// 사건 하나를 남긴다. 실패해도 조용히 넘어간다.
    pub fn record(&self, event: Event) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }
}

/// 한 줄에 하나씩, 앞에 시각을 붙여 적는다(JSON Lines).
///
/// 줄마다 독립이라 도중에 앱이 죽어도 그때까지 적힌 것은 읽을 수 있다.
fn write_line(writer: &mut BufWriter<File>, event: &Event) -> std::io::Result<()> {
    let now: DateTime<Local> = Local::now();

    let mut line = json!({
        "at": now.to_rfc3339(),
        "ms": now.timestamp_millis(),
    });

    // 사건의 항목을 시각 옆에 펼쳐 둔다. 분석할 때 중첩을 한 겹 벗기지
    // 않아도 되도록.
    if let (Some(base), Ok(serde_json::Value::Object(fields))) =
        (line.as_object_mut(), serde_json::to_value(event))
    {
        base.extend(fields);
    }

    writeln!(writer, "{line}")?;

    // 줄마다 흘려보낸다. 실증 중에 앱이 강제로 꺼져도 마지막 몇 초를 잃지
    // 않아야 한다 — 그 구간이 대개 가장 알고 싶은 부분이다.
    writer.flush()
}

/// 오늘 날짜의 기록 파일 경로. 폴더가 없으면 만든다.
fn today_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_log_dir().ok()?;
    fs::create_dir_all(&dir).ok()?;

    let name = format!("events-{}.jsonl", Local::now().format("%Y-%m-%d"));
    Some(dir.join(name))
}

/// 사용자에게 보여줄 기록 폴더 위치.
pub fn directory(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_log_dir().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_of(event: Event) -> serde_json::Value {
        serde_json::to_value(event).expect("직렬화되어야 한다")
    }

    #[test]
    fn 사건_이름이_함께_적힌다() {
        let value = line_of(Event::Input {
            gesture: "short",
            held_ms: 180,
        });

        assert_eq!(value["event"], "input");
        assert_eq!(value["gesture"], "short");
        assert_eq!(value["heldMs"], 180);
    }

    #[test]
    fn 실행_기록에는_반응시간과_지나온_자리가_남는다() {
        // 이 둘이 없으면 오선택과 '지나쳐서 한 바퀴 기다림'을 구분할 수 없다.
        let value = line_of(Event::Action {
            cell: ">".into(),
            action: "next".into(),
            reaction_ms: 420,
            steps: 2,
            cycle: 5,
            ok: true,
            error: None,
        });

        assert_eq!(value["reactionMs"], 420);
        assert_eq!(value["steps"], 2);
        assert_eq!(value["cycle"], 5);
        assert_eq!(value["ok"], true);
    }

    #[test]
    fn 간격_변경은_이전값과_새값을_모두_남긴다() {
        let value = line_of(Event::Interval {
            from_ms: 1800,
            to_ms: 1700,
            reason: "최근 반응이 빨라져 1.8초 → 1.7초".into(),
        });

        assert_eq!(value["fromMs"], 1800);
        assert_eq!(value["toMs"], 1700);
    }

    #[test]
    fn 꺼진_기록기는_아무것도_하지_않는다() {
        // 파일을 열지 못해도 앱은 그대로 돌아야 한다.
        let journal = Journal::off();
        journal.record(Event::Session {
            phase: "start",
            version: "0.1.0".into(),
        });
    }
}
