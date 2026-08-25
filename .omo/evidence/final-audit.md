# 최종 감사 — 안드로이드 패키징 (기기 없이 검증 가능한 전체)

## 완료된 것
- 환경: NDK 26.3.11579264, rust aarch64-linux-android 타깃, tauri-cli 2.11.4, cmdline-tools 설치
- 의존성 위기 해결: @devup-ui npm 측 workspace:^ 오배포 → overrides로 정상 버전 고정, TS shim 체인(@typescript/typescript6→native 재수출 누락) 제거하고 typescript 6.0.3 직접 사용
- 코어 추출: crates/hanbeon-core(플랫폼 무관 스캔·적응·기록·프로필·Host 트레이트) + crates/hanbeon-jni, 데스크톱은 path 의존
- 데스크톱 전용 기능(enigo/serialport/rodio/global-shortcut/tray)을 `desktop` 피처 뒤로 이동. `tauri dev -- --features desktop` / `tauri build -- --features desktop`
- 안드로이드: tauri android init + 디버그 APK 빌드 성공 (libhanbeon_lib.so arm64 포함)
- mobile_entry_point 적용, NoopHost 자리표시자로 커맨드 경로 유지

## 빌드 중 만난 것들과 해결
1. aws-lc-sys가 NDK clang을 못 찾음 → NDK toolchain bin을 PATH에 추가
2. gradle의 rust task가 cargo tauri android android-studio-script를 부름 — 죽은 빌드가 남긴
   $TMPDIR/kr.devfive.hanbeon-server-addr 파일 때문에 WebSocket ConnectionRefused → 파일 삭제
3. mobile_entry_point는 attribute 매크로: #[cfg_attr(android, tauri::mobile_entry_point)]
4. run()에 #[cfg(target_os="android")]를 잘못 붙여 데스크톱에서 심볼 소실 → cfg_attr로 수정

## 회귀 상태 (전부 실측)
- cargo test --workspace: 통과 (no-default 45+80, desktop 74+80+30)
- cargo clippy --workspace -D warnings: 양쪽 피처 모두 0 error
- bun run lint (clippy+fmt+typecheck+oxlint): 통과
- bun test: 43 pass 0 fail
- Next.js 프론트 프로덕션 빌드(out/): 성공

## 기기 테스트 전 남은 것
1. 새 안드로이드 기기(AAPI 26+, arm64) 연결 후 adb install -r app-universal-debug.apk
2. 설정 → 앱 → 한번 → 제한된 설정 허용(Android 13+) → 접근성 서비스 ON
3. 스위치(Uno/CDC P/R) 연결해 종단 확인
4. 이후 단계: 오버레이·접근성·USB를 Tauri 플러그인으로 이식(현재는 WebView Activity만 패키징됨)

## 판정
NEEDS-MORE-WORK(기기 테스트 전) — 그러나 기기 없이 가능한 검증은 전부 통과.
APK 산출물: apps/desktop/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk (~191MB, debug)
