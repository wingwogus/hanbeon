//! 트레이 아이콘.
//!
//! 앱이 Accessory 정책으로 동작하면 Dock/작업 전환기에 나타나지 않는다.
//! 스위치 없이도 설정을 열고 앱을 끌 수 있는 경로가 하나는 있어야 한다.

use tauri::App;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;

use crate::window;

pub fn setup(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let settings = MenuItem::with_id(app, "settings", "설정 열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "한번 종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("한번")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                let _ = window::show_settings(app);
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
