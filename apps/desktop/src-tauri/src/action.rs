//! 화면에 그려지는 칸과, 커서가 도는 순서.
//!
//! **칸과 순서는 같지 않다.** `Enter`는 칸이 하나지만 순환에는 두 번 나온다.
//! 이동(`>`, `<`) 다음에 가장 자주 하는 일이 선택이기 때문이다. 이동 칸마다
//! 그 뒤에 `Enter`를 끼워 두면, 어느 쪽으로 옮겼든 한 틱만 기다리면 고를 수 있다.
//!
//! 값은 순환에 한 자리를 더 쓰는 것이다. 기본 4칸이 5자리가 되어 최악 대기가
//! 3틱에서 4틱으로 는다. 실제로 가장 자주 밟는 길(이동 → 선택)이 1틱으로
//! 짧아지므로, 자주 하는 일을 싸게 만들고 드문 일을 비싸게 만드는 교환이다.

use crate::shortcut::Shortcut;
use serde::Serialize;

/// 한 칸을 눌렀을 때 일어나는 일.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Tab — 다음 요소로
    Next,
    /// Shift+Tab — 이전 요소로
    Prev,
    /// Enter — 선택
    Enter,
    /// 설정 화면 진입
    Settings,
    /// 앱별 칸이 보내는 키 조합.
    Shortcut(Shortcut),
}

/// 칸의 갈래. 화면 배치와 순환 순서가 모두 여기서 갈린다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// 포커스를 옮기는 칸. 왼쪽에 세로로 쌓인다.
    Move,
    /// 선택. 오른쪽에 이동 칸 전체 높이로 붙는다.
    Enter,
    /// 앱별 칸. 구분선 아래에 따로 모인다.
    Extra,
    /// 설정 열기. 가장 드물게 쓰므로 맨 아래.
    Settings,
}

/// 화면에 그려지는 칸 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 칸에 크게 적히는 글자.
    pub label: String,
    /// 커서가 왔을 때 아래에 작게 적히는 이름.
    pub name: String,
    pub kind: Kind,
    pub action: Action,
}

impl Cell {
    pub fn new(label: &str, name: &str, kind: Kind, action: Action) -> Self {
        Self {
            label: label.to_string(),
            name: name.to_string(),
            kind,
            action,
        }
    }
}

/// 어떤 앱에서도 바뀌지 않는 기본 칸.
///
/// 앱별 칸은 `설정` 앞에 끼어든다(`preset::cells_for`).
pub fn base_cells() -> Vec<Cell> {
    vec![
        Cell::new(">", "다음으로", Kind::Move, Action::Next),
        Cell::new("<", "이전으로", Kind::Move, Action::Prev),
        Cell::new("Enter", "선택", Kind::Enter, Action::Enter),
        Cell::new("설정", "설정 열기", Kind::Settings, Action::Settings),
    ]
}

/// 커서가 도는 순서. 값은 `cells`의 자리번호다.
///
/// 이동 칸 하나마다 그 뒤에 `Enter`를 끼운다. 그래서 `Enter`는 칸이 하나여도
/// 순환에는 이동 칸 수만큼 나온다.
pub fn scan_order(cells: &[Cell]) -> Vec<usize> {
    let index_of = |kind: Kind| cells.iter().position(|cell| cell.kind == kind);
    let enter = index_of(Kind::Enter);

    let mut order = Vec::new();

    for (index, cell) in cells.iter().enumerate() {
        if cell.kind == Kind::Move {
            order.push(index);
            if let Some(enter) = enter {
                order.push(enter);
            }
        }
    }

    for (index, cell) in cells.iter().enumerate() {
        if cell.kind == Kind::Extra {
            order.push(index);
        }
    }

    for (index, cell) in cells.iter().enumerate() {
        if cell.kind == Kind::Settings {
            order.push(index);
        }
    }

    // 어떤 칸도 갈래에 걸리지 않는 일은 없어야 하지만, 비어 있는 순서를
    // 돌려주면 커서가 나눗셈에서 터진다.
    if order.is_empty() {
        order.push(0);
    }

    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기본_칸은_이동_둘과_선택과_설정이다() {
        let cells = base_cells();
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0].action, Action::Next);
        assert_eq!(cells[1].action, Action::Prev);
        assert_eq!(cells[2].kind, Kind::Enter);
        assert_eq!(cells[3].kind, Kind::Settings);
    }

    #[test]
    fn 이동_칸마다_뒤에_선택이_끼어든다() {
        let cells = base_cells();
        let order = scan_order(&cells);

        // > → Enter → < → Enter → 설정
        assert_eq!(order, vec![0, 2, 1, 2, 3]);
    }

    #[test]
    fn 선택은_칸이_하나여도_순환에_두_번_나온다() {
        let cells = base_cells();
        let order = scan_order(&cells);
        let enter = cells.iter().position(|c| c.kind == Kind::Enter).unwrap();

        assert_eq!(order.iter().filter(|&&i| i == enter).count(), 2);
    }

    #[test]
    fn 설정은_언제나_순환의_맨_끝이다() {
        let mut cells = base_cells();
        cells.insert(
            3,
            Cell::new("다음 장", "페이지 넘기기", Kind::Extra, Action::Next),
        );

        let order = scan_order(&cells);
        let last = *order.last().unwrap();
        assert_eq!(cells[last].kind, Kind::Settings);
    }

    #[test]
    fn 앱별_칸은_설정_앞에_들어간다() {
        let mut cells = base_cells();
        cells.insert(
            3,
            Cell::new("다음 장", "페이지 넘기기", Kind::Extra, Action::Next),
        );

        let order = scan_order(&cells);
        // > Enter < Enter 다음장 설정
        assert_eq!(order.len(), 6);
        assert_eq!(cells[order[4]].kind, Kind::Extra);
        assert_eq!(cells[order[5]].kind, Kind::Settings);
    }
}
