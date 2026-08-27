//! 안드로이드에서만 존재하는 네이티브 다리.
//!
//! Tauri 웹뷰 UI에서 Kotlin 쪽 서비스(오버레이 등)를 시작하기 위한 최소
//! 통로다. JVM/Activity는 wry·tao의 ndk_glue가 관리하므로 그 경로를 쓴다.
//! (ndk-context 크레이트는 tauri 스택이 초기화하지 않아 금지.)

use tauri::AppHandle;

/// OverlayService.start(activity)를 부른다. 오버레이 권한이 없으면
/// Android가 조용히 무시할 수 있으므로 결과는 기기 로그로 확인한다.
#[tauri::command]
pub fn start_overlay_service(_app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        start_service_internal()
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("오버레이 서비스는 안드로이드 전용이다.".to_string())
    }
}

#[tauri::command]
pub fn transport_status_snapshot() -> Result<serde_json::Value, String> {
    call_setup("transportStatusSnapshot", None)
}

#[tauri::command]
pub fn ble_setup_snapshot() -> Result<serde_json::Value, String> {
    call_setup("bleSetupSnapshot", None)
}

#[tauri::command]
pub fn ble_setup_request_permission() -> Result<serde_json::Value, String> {
    call_setup("bleSetupRequestPermission", None)
}

#[tauri::command]
pub fn ble_setup_scan() -> Result<serde_json::Value, String> {
    call_setup("bleSetupScan", None)
}

#[tauri::command]
pub fn ble_setup_select(token: String) -> Result<serde_json::Value, String> {
    call_setup("bleSetupSelect", Some(token))
}

#[tauri::command]
pub fn ble_setup_revoke() -> Result<serde_json::Value, String> {
    call_setup("bleSetupRevoke", None)
}

fn call_setup(method: &str, token: Option<String>) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        call_setup_internal(method, token)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (method, token);
        Err("블루투스 스위치 설정은 안드로이드 전용이다.".to_string())
    }
}

#[cfg(target_os = "android")]
fn start_service_internal() -> Result<(), String> {
    use jni::objects::{JClass, JObject, JValue};

    eprintln!("[bridge] start_overlay_service 호출됨");

    let context = tauri::tao::platform::android::prelude::main_android_context()
        .ok_or_else(|| "Android 컨텍스트가 아직 준비되지 않았습니다.".to_string())?;

    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }
        .map_err(|e| format!("JavaVM 획득 실패: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("JVM 스레드 부착 실패: {e}"))?;

    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };

    // OverlayService.class
    let class_name = env
        .new_string("kr.devfive.hanbeon.OverlayService")
        .map_err(|e| format!("문자열 생성 실패: {e}"))?;
    let class_obj = {
        let system_class = JClass::from(
            env.find_class("java/lang/Class")
                .map_err(|e| format!("Class 클래스 로딩 실패: {e}"))?,
        );
        env.call_static_method(
            system_class,
            "forName",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name)],
        )
        .map_err(|e| format!("클래스 로딩 실패: {e}"))?
        .l()
        .map_err(|e| format!("클래스 객체 변환 실패: {e}"))?
    };

    // Intent(activity, OverlayService.class)
    let intent = env
        .new_object(
            "android/content/Intent",
            "(Landroid/content/Context;Ljava/lang/Class;)V",
            &[JValue::Object(&activity), JValue::Object(&class_obj)],
        )
        .map_err(|e| format!("Intent 생성 실패: {e}"))?;

    env.call_method(
        &activity,
        "startForegroundService",
        "(Landroid/content/Intent;)Landroid/content/ComponentName;",
        &[JValue::Object(&intent)],
    )
    .map_err(|e| format!("오버레이 서비스 시작 실패: {e}"))?;

    Ok(())
}

#[cfg(target_os = "android")]
fn call_setup_internal(method: &str, token: Option<String>) -> Result<serde_json::Value, String> {
    use jni::objects::{JObject, JString, JValue};

    let context = tauri::tao::platform::android::prelude::main_android_context()
        .ok_or_else(|| "Android 컨텍스트가 아직 준비되지 않았습니다.".to_string())?;
    let vm = unsafe { jni::JavaVM::from_raw(context.java_vm.cast()) }
        .map_err(|e| format!("JavaVM 획득 실패: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("JVM 스레드 부착 실패: {e}"))?;
    let activity = unsafe { JObject::from_raw(context.context_jobject.cast()) };

    let value = if let Some(token) = token {
        let token = env
            .new_string(token)
            .map_err(|e| format!("스위치를 고르지 못했습니다. ({e})"))?;
        env.call_method(
            &activity,
            method,
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&token)],
        )
    } else {
        env.call_method(&activity, method, "()Ljava/lang/String;", &[])
    }
    .map_err(|e| format!("설정 상태를 읽지 못했습니다. ({e})"))?;

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
        return Err("설정 상태를 읽지 못했습니다.".to_string());
    }

    let raw = value
        .l()
        .map_err(|e| format!("설정 상태를 읽지 못했습니다. ({e})"))?;
    let text: String = env
        .get_string(&JString::from(raw))
        .map_err(|e| format!("설정 상태를 읽지 못했습니다. ({e})"))?
        .into();
    serde_json::from_str(&text).map_err(|e| format!("설정 상태를 읽지 못했습니다. ({e})"))
}
