//! Hana Cloud 앱 프로필을 스캔 칸으로 바꾼다.
//!
//! 배포 데이터는 `app_registry`에서 검증한 뒤 이 경계를 통과한다. 이 모듈은
//! 다운로드나 파일 접근을 하지 않고, 이미 메모리에 있는 shortcut만 해석한다.

use crate::action::{Action, Cell, Kind};
use crate::app_registry::RegistryPreset;
use crate::shortcut;

/// 앱별 칸의 상한. 한 바퀴가 지나치게 길어지지 않게 registry schema와 함께
/// 같은 값으로 고정한다.
pub const MAX_EXTRAS: usize = 3;

struct Extra {
    label: String,
    name: String,
    shortcut: shortcut::Shortcut,
}

pub struct PresetSelection {
    pub key: String,
    pub registry_id: String,
    pub name: String,
    extras: Vec<Extra>,
}

impl PresetSelection {
    pub fn from_registry(preset: RegistryPreset) -> Option<Self> {
        let extras: Vec<_> = preset
            .actions
            .into_iter()
            .take(MAX_EXTRAS)
            .filter_map(|action| {
                shortcut::parse(&action.shortcut).map(|shortcut| Extra {
                    label: action.label,
                    name: action.name,
                    shortcut,
                })
            })
            .collect();
        if extras.is_empty() {
            return None;
        }

        Some(Self {
            key: format!("hana-cloud:{}:{}", preset.id, preset.sha256),
            registry_id: preset.id,
            name: preset.name,
            extras,
        })
    }
}

/// 앞 4칸에 프로필의 칸을 붙인 전체 스캔 순서.
pub fn cells_for(preset: Option<&PresetSelection>) -> Vec<Cell> {
    let mut cells = crate::action::base_cells();
    let Some(preset) = preset else {
        return cells;
    };

    // 설정은 언제나 맨 끝이다. 앱별 칸은 그 앞에 끼운다.
    let settings = cells
        .iter()
        .position(|cell| cell.kind == Kind::Settings)
        .unwrap_or(cells.len());
    for (offset, extra) in preset.extras.iter().enumerate() {
        cells.insert(
            settings + offset,
            Cell::new(
                &extra.label,
                &extra.name,
                Kind::Extra,
                Action::Shortcut(extra.shortcut.clone()),
            ),
        );
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_registry::ResolvedAction;

    fn registry_preset() -> RegistryPreset {
        RegistryPreset {
            id: "pdf-viewer".into(),
            name: "PDF 뷰어".into(),
            sha256: "7".repeat(64),
            actions: vec![
                ResolvedAction {
                    label: "다음 장".into(),
                    name: "페이지 넘기기".into(),
                    shortcut: "pagedown".into(),
                },
                ResolvedAction {
                    label: "이전 장".into(),
                    name: "앞 페이지로".into(),
                    shortcut: "pageup".into(),
                },
            ],
        }
    }

    #[test]
    fn 프로필이_없으면_앞_네_칸_그대로다() {
        assert_eq!(cells_for(None).len(), 4);
    }

    #[test]
    fn registry_action을_순서대로_붙인다() {
        let selection = PresetSelection::from_registry(registry_preset()).unwrap();
        let cells = cells_for(Some(&selection));

        assert_eq!(cells.len(), 6);
        assert_eq!(cells[3].label, "다음 장");
        assert_eq!(cells[4].label, "이전 장");
        assert_eq!(selection.registry_id, "pdf-viewer");
        assert!(selection.key.contains(&"7".repeat(64)));
    }

    #[test]
    fn 앱별_칸을_붙여도_설정이_맨_끝이다() {
        let selection = PresetSelection::from_registry(registry_preset()).unwrap();
        let cells = cells_for(Some(&selection));
        assert_eq!(cells.last().map(|cell| cell.kind), Some(Kind::Settings));
    }

    #[test]
    fn 해석할_수_없는_action뿐이면_프로필을_적용하지_않는다() {
        let mut preset = registry_preset();
        preset.actions = vec![ResolvedAction {
            label: "위험".into(),
            name: "알 수 없는 키".into(),
            shortcut: "shell+command".into(),
        }];

        assert!(PresetSelection::from_registry(preset).is_none());
    }
}
