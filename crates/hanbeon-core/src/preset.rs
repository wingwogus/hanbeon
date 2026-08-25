//! 앱별 칸 프리셋.
//!
//! 4칸만으로는 아예 닿을 수 없는 동작이 있다. PDF 뷰어의 페이지 넘김은
//! `PageDown`이고, 음악 앱의 재생 제어는 미디어 키다. 둘 다 `Tab`·`Enter`로는
//! 도달할 수 없어서, 그 앱에서는 컨트롤러가 사실상 아무 쓸모가 없었다.
//!
//! 프리셋은 내장으로만 둔다. 사용자가 키 조합을 직접 편집하는 화면은 스위치
//! 하나로 조작하기에 너무 복잡하다. 대신 설정에서 통째로 끌 수 있다.

use crate::action::{Action, Cell, Kind};
use crate::shortcut;

/// 앱별 칸의 상한.
///
/// 칸이 늘면 한 바퀴가 길어지고 최악 대기가 그만큼 늘어난다. 머무름 덕분에
/// 같은 칸을 반복해 쓰는 값은 싸지만, 처음 그 칸에 닿는 값은 매번 치른다.
pub const MAX_EXTRAS: usize = 3;

/// 한 앱에 붙일 칸의 정의. `keys`는 `shortcut::parse`가 읽는 표기다.
struct Extra {
    label: &'static str,
    name: &'static str,
    keys: &'static str,
}

pub struct Preset {
    /// 상태 줄에 그대로 뜨는 이름.
    pub name: &'static str,
    /// macOS 번들 식별자.
    bundle_ids: &'static [&'static str],
    extras: &'static [Extra],
}

/// 내장 프리셋.
///
/// 되돌릴 수 없는 동작은 넣지 않는다(PRD 원칙 3). PDF의 '파일 열기'는 파일
/// 선택 대화상자를 띄우는데, 그 대화상자는 4칸으로 빠져나올 수 없어서 뺐다.
static PRESETS: &[Preset] = &[
    Preset {
        name: "PDF 뷰어",
        bundle_ids: &["com.apple.Preview", "com.adobe.Reader"],
        extras: &[
            // 서로 되돌리기가 되도록 붙여 둔다. 잘못 넘겼을 때 한 칸만
            // 움직이면 되돌아온다.
            Extra {
                label: "다음 장",
                name: "페이지 넘기기",
                keys: "pagedown",
            },
            Extra {
                label: "이전 장",
                name: "앞 페이지로",
                keys: "pageup",
            },
        ],
    },
    Preset {
        name: "음악 앱",
        bundle_ids: &["com.apple.Music", "com.spotify.client"],
        extras: &[
            Extra {
                label: "재생/멈춤",
                name: "한 번 더 누르면 되돌아옴",
                keys: "mediaplaypause",
            },
            Extra {
                label: "다음 곡",
                name: "다음 곡으로",
                keys: "medianexttrack",
            },
            Extra {
                label: "이전 곡",
                name: "이전 곡으로",
                keys: "mediaprevtrack",
            },
        ],
    },
];

/// 이 앱에 붙일 프리셋.
pub fn for_bundle(bundle_id: &str) -> Option<&'static Preset> {
    PRESETS
        .iter()
        .find(|preset| preset.bundle_ids.contains(&bundle_id))
}

/// 앞 4칸에 프리셋의 칸을 붙인 전체 스캔 순서.
///
/// 해석하지 못한 키는 칸을 만들지 않는다. 없는 키를 아무 키로 대신 보내면
/// 사용자는 익힌 자리에서 다른 동작을 겪게 된다.
pub fn cells_for(preset: Option<&Preset>) -> Vec<Cell> {
    let mut cells = crate::action::base_cells();

    let Some(preset) = preset else {
        return cells;
    };

    // 설정은 언제나 맨 끝이다. 앱별 칸은 그 앞에 끼운다.
    let settings = cells
        .iter()
        .position(|cell| cell.kind == Kind::Settings)
        .unwrap_or(cells.len());

    let mut at = settings;
    for extra in preset.extras.iter().take(MAX_EXTRAS) {
        match shortcut::parse(extra.keys) {
            Some(parsed) => {
                cells.insert(
                    at,
                    Cell::new(
                        extra.label,
                        extra.name,
                        Kind::Extra,
                        Action::Shortcut(parsed),
                    ),
                );
                at += 1;
            }
            None => eprintln!(
                "'{}' 프리셋의 '{}' 칸을 만들지 못했습니다. 키 표기 '{}'를 해석할 수 없습니다.",
                preset.name, extra.label, extra.keys
            ),
        }
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 프리셋이_없으면_앞_네_칸_그대로다() {
        assert_eq!(cells_for(None).len(), 4);
    }

    #[test]
    fn pdf_뷰어는_페이지_넘김_두_칸을_붙인다() {
        let cells = cells_for(for_bundle("com.apple.Preview"));
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[3].label, "다음 장");
        assert_eq!(cells[4].label, "이전 장");
    }

    #[test]
    fn 앱별_칸을_붙여도_설정이_맨_끝이다() {
        for bundle in ["com.apple.Preview", "com.apple.Music"] {
            let cells = cells_for(for_bundle(bundle));
            assert_eq!(cells.last().map(|cell| cell.kind), Some(Kind::Settings));
        }
    }

    #[test]
    fn 음악_앱은_재생_제어_세_칸을_붙인다() {
        let cells = cells_for(for_bundle("com.spotify.client"));
        assert_eq!(cells.len(), 7);
    }

    #[test]
    fn 기본_칸은_어떤_프리셋에서도_같은_순서다() {
        let base = crate::action::base_cells();
        for bundle in ["com.apple.Preview", "com.apple.Music"] {
            let cells = cells_for(for_bundle(bundle));
            let kept: Vec<_> = cells
                .iter()
                .filter(|cell| cell.kind != Kind::Extra)
                .cloned()
                .collect();
            assert_eq!(kept, base);
        }
    }

    #[test]
    fn 모르는_앱에는_붙이지_않는다() {
        assert!(for_bundle("com.example.unknown").is_none());
    }

    #[test]
    fn 내장_프리셋의_키는_모두_해석된다() {
        // 해석되지 않는 표기를 넣어 두면 그 칸이 조용히 사라진다.
        for preset in PRESETS {
            assert!(
                preset.extras.len() <= MAX_EXTRAS,
                "'{}'의 칸이 상한을 넘었습니다",
                preset.name
            );
            for extra in preset.extras {
                assert!(
                    shortcut::parse(extra.keys).is_some(),
                    "'{}'의 '{}' 키 표기를 해석할 수 없습니다",
                    preset.name,
                    extra.keys
                );
            }
        }
    }
}
