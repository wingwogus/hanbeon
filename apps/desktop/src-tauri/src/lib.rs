//! '한번' 데스크톱 코어.
//!
//! 스캔 상태기계·입력 판정·키 주입은 프론트가 아니라 이 코어에 둔다.
//! floating 창이 가려지거나 렌더링이 지연돼도 주사 간격이 흔들리면 안 되고,
//! 스위치 입력 판정(짧게/길게)은 대상 앱으로 키를 보내는 지점과 같은 쪽에
//! 있어야 지연을 예측할 수 있기 때문이다.

#[cfg(target_os = "android")]
mod android_bridge;
mod app_registry;
#[cfg(feature = "desktop")]
pub mod arduino;
#[cfg(feature = "desktop")]
mod audio;
#[cfg(feature = "desktop")]
mod emit;
#[cfg(feature = "desktop")]
mod firmware;
#[cfg(feature = "desktop")]
pub mod flasher;
pub mod focused_application;
mod foreground;
#[cfg(feature = "desktop")]
mod input;
#[cfg(feature = "desktop")]
mod led;
mod occlusion;
mod preset;
pub mod registry;
mod scan;
#[cfg(feature = "desktop")]
mod tray;
mod window;

use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg_attr(not(feature = "desktop"), allow(unused_imports))]
use std::time::Instant;

use serde::Serialize;
#[cfg(feature = "desktop")]
#[cfg_attr(not(feature = "desktop"), allow(unused_imports))]
use tauri::Emitter;
#[allow(unused_imports)]
use tauri::{AppHandle, Manager, State};

#[cfg(feature = "desktop")]
use audio::Audio;
use hanbeon_core::gesture::SharedDetector;
use hanbeon_core::host::Host;
use hanbeon_core::journal::{Event, Journal};
use hanbeon_core::profile::Profile;
use hanbeon_core::scan::{Scanner, Snapshot};
#[cfg(feature = "desktop")]
use scan::DesktopHost;

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

    #[cfg(feature = "desktop")]
    let previous_key = shared
        .0
        .lock()
        .map(|profile| profile.switch_key.clone())
        .map_err(|_| "설정을 읽지 못했습니다.".to_string())?;

    // 스위치 키가 바뀌면 먼저 붙여본다. 실패하면 키만 되돌리고 나머지는 살린다.
    #[cfg(feature = "desktop")]
    let mut warning = None;
    #[cfg(feature = "desktop")]
    if next.switch_key != previous_key {
        let old = input::configured_code(&previous_key);
        let new = input::configured_code(&next.switch_key);
        if let Err(message) = input::rebind(&app, old, new) {
            next.switch_key = previous_key;
            warning = Some(message);
        }
    }

    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("설정 폴더를 찾지 못했습니다. ({error})"))?;
    next.save(&config_dir)?;

    if let Ok(mut profile) = shared.0.lock() {
        *profile = next.clone();
    }
    scanner.apply_profile();
    if let Ok(mut detector) = detector.lock() {
        detector.set_long_press(Duration::from_millis(next.long_press_ms));
    }

    #[cfg(feature = "desktop")]
    let result = Ok(SaveResult {
        profile: next,
        warning,
    });
    #[cfg(not(feature = "desktop"))]
    let result = Ok(SaveResult {
        profile: next,
        warning: None,
    });
    result
}

/// 기록이 어디에 쌓이는지 사용자가 볼 수 있어야 한다. 어디 있는지 모르는
/// 기록은 지울 수도, 실증 담당자에게 건넬 수도 없다.
#[tauri::command]
fn log_directory(app: AppHandle) -> Result<String, String> {
    app.path()
        .app_log_dir()
        .map(|path| path.display().to_string())
        .map_err(|_| "기록 폴더를 찾지 못했습니다.".to_string())
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
    //
    // Android에는 별도 floating 창이 없다. 컨트롤러는 OverlayService가 올린
    // 네이티브 창이고 설정과 무관하게 계속 떠 있다.
    #[cfg(feature = "desktop")]
    return window::show_floating(&app);
    #[cfg(not(feature = "desktop"))]
    Ok(())
}

// 안드로이드에서는 JVM이 System.loadLibrary 후 이 함수를 부른다.
#[cfg_attr(target_os = "android", tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            // 앱을 보조 도구로 낮춘다. Dock/전환기에 뜨지 않고,
            // 활성 앱을 가로채지 않아 키 주입 대상이 유지된다.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            #[cfg(feature = "desktop")]
            tray::setup(app)?;

            let config_dir = app.path().app_config_dir()?;
            let mut profile = Profile::load(&config_dir);
            // 검증용 통로. 저장된 설정을 건드리지 않고 시작 간격만 바꿔 끼운다.
            if let Some(interval_ms) = scan::interval_override() {
                profile.interval_ms = interval_ms;
                profile.max_interval_ms = profile.max_interval_ms.max(interval_ms);
                profile.sanitize();
            }
            // 설치기가 포트 소유권을 보류할지 결정한다. 데스크톱 전용 경로다.
            #[cfg(feature = "desktop")]
            let needs_onboarding = !profile.onboarded;

            // 창 배치는 프로필을 읽은 다음이어야 한다. 사용자가 옮겨 둔 위치를
            // 모른 채 먼저 띄우면 기본 위치에서 한 번 튄 뒤에 제자리를 찾는다.
            // 안드로이드는 floating 창 배치·non-activating 개념이 없고, 웹뷰가
            // 아직 준비 전일 때 eval하면 "failed to send message"로 죽는다.
            #[cfg(feature = "desktop")]
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

            #[cfg(feature = "desktop")]
            let audio = {
                let a = Audio::spawn();
                a.set_enabled(profile.sound);
                a
            };

            // 실증 지표는 실측으로만 주장할 수 있고, 그러려면 무엇이 언제
            // 일어났는지가 파일로 남아야 한다(PRD 10절).
            let journal = if profile.logging {
                Journal::open(&app.path().app_log_dir()?)
            } else {
                Journal::off()
            };
            journal.record(Event::Session {
                phase: "start",
                version: app.package_info().version.to_string(),
            });

            #[cfg(feature = "desktop")]
            let switch_code = input::configured_code(&profile.switch_key);
            let detector: SharedDetector = Arc::new(Mutex::new(
                hanbeon_core::gesture::GestureDetector::new(Duration::from_millis(
                    profile.long_press_ms,
                )),
            ));

            let profile = Arc::new(Mutex::new(profile));
            #[cfg(feature = "desktop")]
            let host = Arc::new(DesktopHost::new(
                app.handle().clone(),
                audio,
                config_dir,
            ));
            // 안드로이드는 접근성 서비스 플러그인이 Host를 구현한다. 그때까지
            // 커맨드 경로가 컴파일되도록 하는 자리표시자.
            #[cfg(not(feature = "desktop"))]
            let host: Arc<dyn Host> = Arc::new(hanbeon_core::host::NoopHost);

            let scanner = Scanner::new(Arc::clone(&profile), host.clone() as Arc<dyn Host>, journal.clone());
            // 안드로이드는 rustls-platform-verifier의 JNI 초기화가 없어 reqwest
            // 클라이언트가 패닉한다. 하늘구름 프리셋은 데스크톱에서만 당분간.
            #[cfg(feature = "desktop")]
            let registry = app_registry::Registry::spawn(
                app.path().app_cache_dir()?.join("hana-cloud"),
            );
            #[cfg(not(feature = "desktop"))]
            let registry = app_registry::Registry::noop(app.path().app_cache_dir()?);
            let moves = window::MoveWatch::default();

            app.manage(SharedProfile(Arc::clone(&profile)));
            app.manage(Arc::clone(&detector));
            app.manage(scanner.clone());
            app.manage(registry.clone());
            app.manage(moves.clone());

            moves.watch(app.handle().clone(), Arc::clone(&profile));
            foreground::watch(
                app.handle().clone(),
                Arc::clone(&profile),
                scanner.clone(),
                registry,
            );
            #[cfg(feature = "desktop")]
            if let Some(snapshot) = scanner.snapshot() {
                host.sync_led(&snapshot);
            }
            scanner.start();

#[cfg(feature = "desktop")]
        {
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
            // 새 보드는 아직 Hana 펌웨어가 없어 handshake에 답할 수 없다. 최초
            // 온보딩 동안에는 포트를 열지 않고 설치기가 명시적으로 시작될 때까지
            // 소유권을 보류한다. 설치가 끝나면 coordinator가 연결 worker를 시작한다.
            let native_switch = if needs_onboarding {
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
                move |_app, judgement| {
                    scanner.handle(judgement);
                },
            );
            journal.record(Event::Switch {
                state: if registered.is_ok() {
                    "registered"
                } else {
                    "failed"
                },
                key: format!("{switch_code:?}"),
            });
            registered?;

            app.manage(journal);
        }

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
        #[cfg(target_os = "android")]
        android_bridge::start_overlay_service,
        #[cfg(target_os = "android")]
        android_bridge::transport_status_snapshot,
        #[cfg(target_os = "android")]
        android_bridge::ble_setup_snapshot,
        #[cfg(target_os = "android")]
        android_bridge::ble_setup_request_permission,
        #[cfg(target_os = "android")]
        android_bridge::ble_setup_scan,
        #[cfg(target_os = "android")]
        android_bridge::ble_setup_select,
        #[cfg(target_os = "android")]
        android_bridge::ble_setup_revoke,
            scan_snapshot,
            get_profile,
            save_profile,
            open_settings,
            close_settings,
            log_directory,
            #[cfg(feature = "desktop")]
            firmware::list_arduino_candidates,
            #[cfg(feature = "desktop")]
            firmware::probe_arduino_firmware,
            #[cfg(feature = "desktop")]
            firmware::begin_firmware_install,
            #[cfg(feature = "desktop")]
            firmware::cancel_firmware_install
        ])
        .build(tauri::generate_context!())
        .expect("한번 앱을 시작하지 못했습니다");

    app.run(|app, event| {
        // 적응으로 조정된 간격은 메모리에만 있다. 종료할 때 한 번 적어 두어야
        // 다음에 켰을 때 사용자가 익숙해진 속도로 시작한다.
        if let tauri::RunEvent::Exit = event
            && let Some(journal) = app.try_state::<Journal>()
        {
            journal.record(Event::Session {
                phase: "stop",
                version: app.package_info().version.to_string(),
            });
        }

        if let tauri::RunEvent::Exit = event
            && let Some(shared) = app.try_state::<SharedProfile>()
            && let Ok(profile) = shared.0.lock()
            && let Ok(config_dir) = app.path().app_config_dir()
            && let Err(message) = profile.save(&config_dir)
        {
            eprintln!("종료하며 설정을 저장하지 못했습니다. {message}");
        }
    });
}
