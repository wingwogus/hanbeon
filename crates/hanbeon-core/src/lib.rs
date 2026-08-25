//! '한번'의 코어.
//!
//! 여기 있는 것은 **어느 플랫폼에서도 같다.** 스캔 상태기계, 간격 적응, 앱별 칸,
//! 프로필, 이벤트 기록.
//!
//! 같은 코드를 쓰는 이유는 손이 덜 가서가 아니다. 주사 간격과 눌림 판정이
//! 플랫폼마다 미묘하게 달라지면 사용자가 기기를 옮길 때마다 타이밍을 다시 익혀야
//! 한다. 이 제품의 사용자에게 그것은 큰 비용이다.
//!
//! **플랫폼에 닿는 것은 `host::Host` 뒤에 둔다.** 이 crate에 `tauri`·`enigo`·
//! `jni` 같은 것을 들이지 않는다. 들이는 순간 반대쪽 플랫폼에서 통째로 못 쓰게 된다.

pub mod action;
pub mod adapt;
pub mod cue;
pub mod gesture;
pub mod host;
pub mod journal;
pub mod key;
pub mod preset;
pub mod profile;
pub mod scan;
pub mod shortcut;
