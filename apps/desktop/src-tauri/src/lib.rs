//! '한번' 데스크톱 코어.
//!
//! 스캔 상태기계·입력 판정·키 주입은 프론트가 아니라 이 코어에 둔다.
//! floating 창이 가려지거나 렌더링이 지연돼도 주사 간격이 흔들리면 안 되고,
//! 스위치 입력 판정(짧게/길게)은 대상 앱으로 키를 보내는 지점과 같은 쪽에
//! 있어야 지연을 예측할 수 있기 때문이다.

mod action;
mod adapt;
pub mod arduino;
mod audio;
mod emit;
mod firmware;
pub mod flasher;
mod foreground;
mod input;
mod journal;
mod led;
mod occlusion;
mod preset;
mod profile;
pub mod registry;
mod scan;
mod shortcut;
mod tray;
mod window;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use audio::Audio;
use input::{GestureDetector, SharedDetector};
use profile::Profile;
use scan::{Scanner, Snapshot};

/// 코어가 들고 있는 현재 프로필. 설정 화면과 적응 로직이 함께 쓴다.
struct SharedProfile(Arc<Mutex<Profile>>);

/// 설정 저장 결과.
///
/// 일부만 실패할 수 있어서 경고를 함께 돌려준다. 스위치 키 등록이 실패했다고
/// 나머지 설정까지 버리면 사용자는 방금 맞춘 속도를 다시 잡아야 한다.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResult {
    profile: Profile,
    warning: Option<String>,
}

/// 프론트가 창을 띄운 직후 현재 커서를 맞추기 위해 호출한다.
/// 이벤트만으로는 첫 틱이 올 때까지 화면이 비어 있게 된다.
#[tauri::command]
fn scan_snapshot(scanner: State<'_, Scanner>) -> Result<Snapshot, String> {
    scanner
        .snapshot()
        .ok_or_else(|| "스캔 상태를 읽지 못했습니다.".to_string())
}

#[tauri::command]
fn get_profile(shared: State<'_, SharedProfile>) -> Result<Profile, String> {
    shared
        .0
        .lock()
        .map(|profile| profile.clone())
        .map_err(|_| "설정을 읽지 못했습니다.".to_string())
}

#[tauri::command]
fn save_profile(
    app: AppHandle,
    mut next: Profile,
    shared: State<'_, SharedProfile>,
    scanner: State<'_, Scanner>,
    detector: State<'_, SharedDetector>,
) -> Result<SaveResult, String> {
    next.sanitize();

    let previous_key = shared
        .0
        .lock()
        .map(|profile| profile.switch_key.clone())
        .map_err(|_| "설정을 읽지 못했습니다.".to_string())?;

    // 스위치 키가 바뀌면 먼저 붙여본다. 실패하면 키만 되돌리고 나머지는 살린다.
    let mut warning = None;
    if next.switch_key != previous_key {
        let old = input::configured_code(&previous_key);
        let new = input::configured_code(&next.switch_key);
        if let Err(message) = input::rebind(&app, old, new) {
            next.switch_key = previous_key;
            warning = Some(message);
        }
    }

    next.save(&app)?;

    if let Ok(mut profile) = shared.0.lock() {
        *profile = next.clone();
    }
    scanner.apply_profile();
    if let Ok(mut detector) = detector.lock() {
        detector.set_long_press(Duration::from_millis(next.long_press_ms));
    }

    Ok(SaveResult {
        profile: next,
        warning,
    })
}

/// 기록이 어디에 쌓이는지 사용자가 볼 수 있어야 한다. 어디 있는지 모르는
/// 기록은 지울 수도, 실증 담당자에게 건넬 수도 없다.
#[tauri::command]
fn log_directory(app: AppHandle) -> Result<String, String> {
    journal::directory(&app)
        .map(|path| path.display().to_string())
        .ok_or_else(|| "기록 폴더를 찾지 못했습니다.".to_string())
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    window::show_settings(&app)
}

#[tauri::command]
fn close_settings(app: AppHandle) -> Result<(), String> {
    window::hide_settings(&app)?;
    // 설정(또는 온보딩)이 닫히면 스캔 오버레이가 곧바로 보여야 한다.
    // 설치 모드에서 숨겨진 floating도 이 호출로 되살아난다.
    window::show_floating(&app)
}

pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            // 앱을 보조 도구로 낮춘다. Dock/전환기에 뜨지 않고,
            // 활성 앱을 가로채지 않아 키 주입 대상이 유지된다.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::setup(app)?;

            let mut profile = Profile::load(app.handle());
            // 검증용 통로. 저장된 설정을 건드리지 않고 시작 간격만 바꿔 끼운다.
            if let Some(interval_ms) = scan::interval_override() {
                profile.interval_ms = interval_ms;
                profile.max_interval_ms = profile.max_interval_ms.max(interval_ms);
                profile.sanitize();
            }

            let arduino_startup = firmware::startup_mode();
            if arduino_startup == firmware::StartupMode::Setup {
                profile.onboarded = false;
            }

            // 창 배치는 프로필을 읽은 다음이어야 한다. 사용자가 옮겨 둔 위치를
            // 모른 채 먼저 띄우면 기본 위치에서 한 번 튄 뒤에 제자리를 찾는다.
            if let Some(floating) = app.get_webview_window("floating") {
                window::prepare_floating(&floating, profile.window_position)?;
            }

            if std::env::var("HANBEON_LOG").is_ok() {
                eprintln!(
                    "[profile] 간격 {}ms (범위 {}~{}ms), 적응 {}, 수동고정 {}, 길게누름 {}ms, 소리 {}, 초기설정 {}",
                    profile.interval_ms,
                    profile.min_interval_ms,
                    profile.max_interval_ms,
                    profile.adaptive,
                    profile.manual_lock,
                    profile.long_press_ms,
                    profile.sound,
                    profile.onboarded,
                );
            }

            let audio = Audio::spawn();
            audio.set_enabled(profile.sound);

            // 실증 지표는 실측으로만 주장할 수 있고, 그러려면 무엇이 언제
            // 일어났는지가 파일로 남아야 한다(PRD 10절).
            let journal = if profile.logging {
                journal::Journal::open(app.handle())
            } else {
                journal::Journal::off()
            };
            journal.record(journal::Event::Session {
                phase: "start",
                version: app.package_info().version.to_string(),
            });

            let switch_code = input::configured_code(&profile.switch_key);
            let detector: SharedDetector = Arc::new(Mutex::new(GestureDetector::new(
                Duration::from_millis(profile.long_press_ms),
            )));

            let profile = Arc::new(Mutex::new(profile));
            let scanner = Scanner::new(Arc::clone(&profile), audio, journal.clone());
            let moves = window::MoveWatch::default();

            app.manage(SharedProfile(Arc::clone(&profile)));
            app.manage(Arc::clone(&detector));
            app.manage(scanner.clone());
            app.manage(moves.clone());

            moves.watch(app.handle().clone(), Arc::clone(&profile));
            foreground::watch(app.handle().clone(), Arc::clone(&profile), scanner.clone());
            scanner.start(app.handle().clone());

            // Native serial starts at app launch. P/R edges share GestureDetector
            // with the HID/F13 fallback below; Accessibility is used only later
            // when Scanner::handle injects into another app.
            let native_app = app.handle().clone();
            let native_detector = Arc::clone(&detector);
            let native_scanner = scanner.clone();
            let spawn_switch = move || {
                let lifecycle_app = native_app.clone();
                let switch_app = native_app.clone();
                let switch_detector = Arc::clone(&native_detector);
                let switch_scanner = native_scanner.clone();
                arduino::ArduinoSwitch::spawn(
                    arduino::ReconnectPolicy::default(),
                    move |event| {
                        if std::env::var("HANBEON_LOG").is_ok() {
                            eprintln!("[arduino] lifecycle: {event:?}");
                        }
                        if let Err(error) =
                            lifecycle_app.emit(arduino::EVENT_LIFECYCLE, event)
                        {
                            eprintln!("Arduino lifecycle event를 보내지 못했습니다. {error}");
                        }
                    },
                    move |event| {
                        arduino::route_switch_event(
                            &switch_detector,
                            event,
                            Instant::now(),
                            |judgement| {
                                input::announce(&switch_app, judgement);
                                switch_scanner.handle(&switch_app, judgement);
                            },
                        );
                    },
                )
            };
            let native_switch = if arduino_startup == firmware::StartupMode::Setup {
                if let Some(floating) = app.get_webview_window("floating") {
                    floating.hide()?;
                }
                window::show_settings(app.handle())?;
                arduino::ArduinoCoordinator::for_installer(spawn_switch)
            } else {
                arduino::ArduinoCoordinator::new(spawn_switch)
            };
            app.manage(native_switch);
            app.manage(firmware::FirmwareInstaller::default());

            let registered = input::register(
                app.handle(),
                detector,
                switch_code,
                move |app, judgement| {
                    scanner.handle(app, judgement);
                },
            );
            journal.record(journal::Event::Switch {
                state: if registered.is_ok() {
                    "registered"
                } else {
                    "failed"
                },
                key: format!("{switch_code:?}"),
            });
            registered?;

            app.manage(journal);

            Ok(())
        })
        .on_window_event(|window, event| {
            // 설정 창을 닫아도 앱은 살아 있어야 한다. 창을 파괴하는 대신 숨기고
            // 보조 도구 정책으로 되돌린다. 그러지 않으면 이후 floating 컨트롤러가
            // 대상 앱의 포커스를 계속 뺏는다.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "settings"
            {
                api.prevent_close();
                let _ = window::hide_settings(window.app_handle());
            }

            // 사용자가 끌어 옮긴 위치를 기억한다. 저장과 활성 상태 복구는
            // 이동이 멎은 뒤에 한 번만 일어난다(`MoveWatch`).
            if let tauri::WindowEvent::Moved(position) = event
                && window.label() == "floating"
                && let Some(moves) = window.app_handle().try_state::<window::MoveWatch>()
            {
                moves.note((position.x, position.y));
            }
        })
        .invoke_handler(tauri::generate_handler![
            scan_snapshot,
            get_profile,
            save_profile,
            open_settings,
            close_settings,
            log_directory,
            firmware::list_arduino_candidates,
            firmware::probe_arduino_firmware,
            firmware::begin_firmware_install,
            firmware::cancel_firmware_install
        ])
        .build(tauri::generate_context!())
        .expect("한번 앱을 시작하지 못했습니다");

    app.run(|app, event| {
        // 적응으로 조정된 간격은 메모리에만 있다. 종료할 때 한 번 적어 두어야
        // 다음에 켰을 때 사용자가 익숙해진 속도로 시작한다.
        if let tauri::RunEvent::Exit = event
            && let Some(journal) = app.try_state::<journal::Journal>()
        {
            journal.record(journal::Event::Session {
                phase: "stop",
                version: app.package_info().version.to_string(),
            });
        }

        if let tauri::RunEvent::Exit = event
            && let Some(shared) = app.try_state::<SharedProfile>()
            && let Ok(profile) = shared.0.lock()
            && let Err(message) = profile.save(app)
        {
            eprintln!("종료하며 설정을 저장하지 못했습니다. {message}");
        }
    });
}
