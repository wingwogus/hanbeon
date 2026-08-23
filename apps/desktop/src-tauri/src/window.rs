//! floating 컨트롤러 창의 배치와 플랫폼별 창 속성.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

use crate::profile::Profile;

/// 화면 가장자리에서 띄울 여백(논리 px).
const EDGE_MARGIN: f64 = 24.0;

/// 저장된 위치를 되살릴 때 화면 안에 최소한 이만큼은 남아 있어야 한다(물리 px).
const MIN_VISIBLE: i32 = 80;

/// 컨트롤러 창의 가로 폭(논리 px).
const WIDTH: f64 = 360.0;

// 높이는 화면의 CSS 배치와 짝이 맞아야 한다. 어긋나면 칸이 잘리거나 빈 자리가
// 남는다. 아래 값은 `app/page.tsx`의 배치와 같은 뜻이다.
/// 손잡이·남은 시간 막대·상태 줄·바깥 여백·그 사이 간격을 모두 더한 값.
const CHROME: f64 = 96.0;
/// 이동 두 칸이 세로로 쌓인 블록(오른쪽의 선택 칸이 같은 높이를 쓴다).
const MOVE_BLOCK: f64 = 128.0;
/// 앱별 칸 구분선과 그 위아래 간격.
const DIVIDER: f64 = 28.0;
/// 칸 한 줄과 칸 사이 여백.
const ROW: f64 = 60.0;
const GAP: f64 = 8.0;
/// 설정 줄. 가장 드물게 쓰므로 다른 칸보다 낮다.
const SETTINGS_ROW: f64 = 48.0;

/// 이동이 이 시간 동안 멎으면 드래그가 끝난 것으로 본다.
const SETTLE: Duration = Duration::from_millis(400);

/// 이동이 멎었는지 확인하는 주기.
const WATCH_TICK: Duration = Duration::from_millis(100);

pub fn prepare_floating(window: &WebviewWindow, saved: Option<(i32, i32)>) -> tauri::Result<()> {
    // 높이를 계산식으로 한 번 맞춘다. tauri.conf.json의 값과 여기 계산이
    // 어긋나면 첫 화면에서 맨 아래 칸이 잘린 채로 시작한다.
    fit_cells(window, 0)?;

    match saved {
        Some(position) => restore(window, position)?,
        None => place_bottom_right(window)?,
    }
    make_non_activating(window);
    Ok(())
}

/// 기본 위치는 주 모니터 우하단.
fn place_bottom_right(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };

    let scale = monitor.scale_factor();
    let screen = monitor.size();
    let origin = monitor.position();
    let size = window.outer_size()?;
    let margin = (EDGE_MARGIN * scale) as i32;

    let x = origin.x + screen.width as i32 - size.width as i32 - margin;
    let y = origin.y + screen.height as i32 - size.height as i32 - margin;

    window.set_position(PhysicalPosition::new(x, y))
}

/// 칸 수에 맞춰 창 높이를 맞춘다.
///
/// **좌상단을 고정하고 아래로 자란다.** 그래야 앞 4칸의 화면상 자리가 그대로
/// 유지된다 — 사용자는 자리로 동작을 기억하므로, 칸이 늘 때마다 4칸이 움직이면
/// 익힌 것이 매번 무효가 된다.
///
/// 대신 아래로 자라다 화면 밖으로 나갈 수 있다. 그때만 창을 위로 올린다.
pub fn fit_cells(window: &WebviewWindow, extras: usize) -> tauri::Result<()> {
    let mut height = CHROME + MOVE_BLOCK;

    if extras > 0 {
        let rows = extras.div_ceil(2) as f64;
        height += DIVIDER + rows * ROW + (rows - 1.0) * GAP + GAP;
    }
    height += GAP + SETTINGS_ROW;

    window.set_size(LogicalSize::new(WIDTH, height))?;
    nudge_onto_screen(window)
}

/// 창이 화면 아래로 넘쳤으면 넘친 만큼만 올린다.
fn nudge_onto_screen(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };

    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let origin = monitor.position();
    let bottom = origin.y + monitor.size().height as i32;

    let overflow = position.y + size.height as i32 - bottom;
    if overflow <= 0 {
        return Ok(());
    }

    window.set_position(PhysicalPosition::new(position.x, position.y - overflow))
}

/// 사용자가 옮겨 둔 위치로 되돌린다.
///
/// 모니터를 떼거나 해상도가 바뀌면 지난번 위치가 화면 밖일 수 있다. 그대로
/// 두면 창이 보이지 않고, 스위치만 쓰는 사용자는 창을 되찾을 수단이 없다.
fn restore(window: &WebviewWindow, position: (i32, i32)) -> tauri::Result<()> {
    if on_screen(window, position)? {
        window.set_position(PhysicalPosition::new(position.0, position.1))
    } else {
        place_bottom_right(window)
    }
}

/// 이 위치에 두었을 때 어느 모니터에든 창이 충분히 걸치는지.
fn on_screen(window: &WebviewWindow, (x, y): (i32, i32)) -> tauri::Result<bool> {
    let size = window.outer_size()?;
    let width = size.width as i32;
    let height = size.height as i32;

    // 창이 화면보다 작을 수 있으므로 요구치는 창 크기로 한 번 더 깎는다.
    let need_x = MIN_VISIBLE.min(width);
    let need_y = MIN_VISIBLE.min(height);

    for monitor in window.available_monitors()? {
        let origin = monitor.position();
        let screen = monitor.size();

        let overlap_x = (x + width).min(origin.x + screen.width as i32) - x.max(origin.x);
        let overlap_y = (y + height).min(origin.y + screen.height as i32) - y.max(origin.y);

        if overlap_x >= need_x && overlap_y >= need_y {
            return Ok(true);
        }
    }

    Ok(false)
}

/// floating 창이 대상 앱의 포커스를 뺏으면 안 된다. 포커스를 가져가는 순간
/// 우리가 주입한 Tab/Enter가 대상 앱이 아니라 우리 창으로 들어간다.
///
/// macOS는 `ActivationPolicy::Accessory`(lib.rs)로 앱 자체의 활성화를 먼저 막는다.
/// 그것만으로 창 클릭 시 활성화가 남는다면 NSPanel + NonactivatingPanel로 승격한다.
#[allow(unused_variables)]
fn make_non_activating(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        // TODO(M1-win): WS_EX_NOACTIVATE 추가. Windows 실기에서 검증한다.
    }
}

/// 활성 상태를 직전 앱에 돌려준다.
///
/// 창을 끌면 macOS는 우리 앱을 활성 앱으로 올린다. 그대로 두면 이후 주입한
/// Tab이 대상 앱이 아니라 우리 창으로 들어가고, 스위치만 쓰는 사용자는 다른
/// 앱을 다시 클릭할 수단이 없어 조작 자체가 막힌다. 창을 옮길 수 있게 된
/// 이상, 옮긴 뒤 원래대로 돌려놓는 것까지가 한 동작이다.
pub fn release_activation(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        // AppKit은 메인 스레드에서만 만질 수 있다.
        let _ = app.run_on_main_thread(|| {
            use objc2::MainThreadMarker;
            use objc2_app_kit::NSApplication;

            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).deactivate();
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

/// 마지막으로 관측한 이동. 시각을 함께 들고 있어야 멎었는지 알 수 있다.
type LastMove = Option<(Instant, (i32, i32))>;

/// 드래그 중 쏟아지는 이동 이벤트를 모아 두는 곳.
///
/// 창을 끄는 동안에는 이동이 픽셀 단위로 들어온다. 그때마다 파일에 쓰면
/// 디스크가 아니라 조작감이 먼저 무너진다. 이동이 멎은 뒤 한 번만 저장한다.
#[derive(Clone, Default)]
pub struct MoveWatch(Arc<Mutex<LastMove>>);

impl MoveWatch {
    pub fn note(&self, position: (i32, i32)) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some((Instant::now(), position));
        }
    }

    /// 이동이 멎었으면 마지막 위치를 꺼낸다.
    fn take_settled(&self) -> Option<(i32, i32)> {
        let mut slot = self.0.lock().ok()?;
        match *slot {
            Some((at, position)) if at.elapsed() >= SETTLE => {
                *slot = None;
                Some(position)
            }
            _ => None,
        }
    }

    /// 이동이 멎기를 기다렸다가 위치를 저장하고 활성 상태를 돌려준다.
    pub fn watch(&self, app: AppHandle, profile: Arc<Mutex<Profile>>) {
        let watch = self.clone();

        thread::spawn(move || {
            loop {
                thread::sleep(WATCH_TICK);

                let Some(position) = watch.take_settled() else {
                    continue;
                };

                // 위치가 실제로 바뀐 경우에만 움직인다. 시작할 때 우리가 부른
                // `set_position`도 이동으로 들어오는데, 그때까지 활성 앱을
                // 건드리면 사용자가 쓰던 창이 이유 없이 뒤로 밀린다.
                let moved = match profile.lock() {
                    Ok(mut profile) if profile.window_position != Some(position) => {
                        profile.window_position = Some(position);
                        if let Err(message) = profile.save(&app) {
                            eprintln!("창 위치를 저장하지 못했습니다. {message}");
                        }
                        true
                    }
                    _ => false,
                };

                if moved {
                    release_activation(&app);
                }
            }
        });
    }
}

/// 설정 창은 floating 컨트롤러와 요구가 정반대다. 조작을 받아야 하므로
/// 반드시 활성화되어야 한다. Accessory 정책인 채로 열면 창은 보이지만
/// 포커스가 가지 않아 아무것도 할 수 없다.
pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "설정 창을 찾을 수 없습니다.".to_string())?;

    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// 설정 창을 닫으면 다시 보조 도구로 내려간다.
/// 그래야 floating 컨트롤러가 대상 앱의 포커스를 뺏지 않는다.
pub fn hide_settings(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "설정 창을 찾을 수 없습니다.".to_string())?;
    window.hide().map_err(|e| e.to_string())?;

    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    Ok(())
}
