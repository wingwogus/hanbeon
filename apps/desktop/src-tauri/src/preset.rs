//! Adapts the downloaded desktop registry to core scan cells.

use hanbeon_core::action::{Action, Cell, Kind};
use hanbeon_core::preset::MAX_EXTRAS;
use hanbeon_core::shortcut;

use crate::app_registry::RegistryPreset;

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

pub fn cells_for(preset: Option<&PresetSelection>) -> Vec<Cell> {
    let mut cells = hanbeon_core::action::base_cells();
    let Some(preset) = preset else {
        return cells;
    };

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
