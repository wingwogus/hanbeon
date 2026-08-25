---
slug: android-tauri-packaging
status: drafting
intent: clear
review_required: true
plan_path: .omo/plans/android-tauri-packaging.md
plan_sha256: e9a612b931a482f1253bb56dcb6aa22c9129e1022e55029bede9dac7f3300d07
review_round_id: momus-r2-android-tauri-packaging
review_round_limit: 5
pending-action: write and review .omo/plans/android-tauri-packaging.md
review:
  momus:
    status: approved
    workspace_root: /Users/wingwogus/orca/workspaces/hanbeon/feat-android-tauri
    runtime_home: null
    target: .omo/plans/android-tauri-packaging.md
    round_id: momus-r2-android-tauri-packaging
    plan_sha256: e9a612b931a482f1253bb56dcb6aa22c9129e1022e55029bede9dac7f3300d07
    launch_id: null
    session: null
    result: r1 REJECT(T분기 성공기준 vs G폴백 모순 — 수정), r2 OKAY(2026-08-24, 세션 st_01a0333b, plan_sha256 e9a612b9...00d07 라이브 검증 일치)
approach: 참고 브랜치(orca/hanbeon feat/android-one-row-controller)의 구조를 현재 워크스페이스로 이식 — 플랫폼 무관 코어를 crates/hanbeon-core로 추출, JNI 다리(crates/hanbeon-jni), 순수 Gradle Kotlin 껍데기(apps/android). Tauri 안드로이드 사용 여부는 열린 소유자 결정(포크 1).
---

# Draft: android-tauri-packaging

## Components (topology ledger)
<!-- id | outcome | status | evidence path -->
- C1 코어 추출 | 데스크톱 src-tauri에서 플랫폼 무관 로직을 crates/hanbeon-core로 분리, 데스크톱 빌드 유지 | active | apps/desktop/src-tauri/src/lib.rs, /Users/wingwogus/orca/hanbeon/crates/hanbeon-core/
- C2 JNI 다리 | hanbeon-core를 안드로이드용 cdylib으로 빌드하는 JNI 래퍼 | active | /Users/wingwogus/orca/hanbeon/crates/hanbeon-jni/src/lib.rs (346줄)
- C3 안드로이드 껍데기 | OverlayService·AccessibilityService·UsbSwitch·MainActivity Kotlin 앱 | active | /Users/wingwogus/orca/hanbeon/apps/android/ (실기 검증 완료 2026-08-24)
- C4 빌드 파이프라인 | NDK 링커 설정, build-core.sh, cargo android 타깃 추가 | active | /Users/wingwogus/orca/hanbeon/scripts/android/build-core.sh
- C5 Tauri 하이브리드 여부 | 설정 UI 등을 Tauri Activity로 갈지 — 포크 1 결과에 따라 active/deferred | deferred | 아래 Findings F3

## Open assumptions (announced defaults)
- 공유 코어 위치 | crates/hanbeon-core 워크스페이스 멤버로 추출 (참고 저장소와 동일 레이아웃) | 두 플랫폼이 같은 타이밍 코드를 쓰게 하려는 제품 원칙(CLAUDE.md) | 되돌림 가능
- minSdk 26, arm64-v8a 우선 | 참고 앱과 동일 | 오버레이 API 요구사항 + 실기 A15 | 되돌림 가능
- architect 자문 라인 | 스킴 (provider disabled 오류 2회) | 직접 탐색+참고 구현이 더 강한 증거 | -

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
<!-- assumption | adopted default | rationale | reversible? -->

## Findings (cited - path:lines)
- F1: 참고 프로젝트는 안드로이드에서 **의도적으로 Tauri를 안 썼다** — /Users/wingwogus/orca/hanbeon/apps/android/README.md "왜 Tauri가 아닌가": 오버레이는 SYSTEM_ALERT_WINDOW 권한의 포그라운드 Service가 TYPE_APPLICATION_OVERLAY 창을 직접 올려야 하고, Tauri 안드로이드 지원은 Activity만 제공.
- F2: 참고 브랜치는 실기 검증까지 완료(2026-08-24, 갤럭시 A15): 스위치→아두이노→USB 시리얼→코어→접근성 서비스→크롬, 4칸 순환·간격 적응·실증 기록 동작. 남은 것: 소리, 설정 화면, 앱별 칸 실행 경로 (apps/android/README.md "아직 없는 것").
- F3: 핵심 교체표(PRD 5.5): Tab 주입→포커스 트리 순회(focusSearch는 입력 포커스용이라 불가, 실기 확인), Enter→performAction(ACTION_CLICK), 되돌리기→GLOBAL_ACTION_BACK, 앱 감시 폴링→TYPE_WINDOW_STATE_CHANGED 이벤트.
- F4: 현재 워크스페이스 코어는 데스크톱 전용 결합: scan.rs(1293줄)가 tauri AppHandle/Emitter 직접 호출(scan.rs:26,584…), profile/journal도 AppHandle 의존(profile.rs:122…). 참고 코어(hanbeon-core 3073줄)는 Host 트레이트(host.rs:42)로 경계 분리 — inject/undo/openSettings/fitCells/cue/publish/save_profile.
- F5: 참고 데스크톱은 이미 hanbeon-core 크레이트를 path 의존으로 재사용(src-tauri/Cargo.toml:17) — 추출 패턴의 선례 존재.
- F6: 로컬 환경: Android SDK 있음(~/Library/Android/sdk, NDK 없음), rustup android 타깃 미설치, NDK 링커 .cargo/config.toml은 새로 작성 필요(참고 저장소도 커밋 안 함).
- F7: 웹 검색(v2.tauri.app, developer.android.com/develop/background-work/services/fgs/changes): Tauri v2는 Android Activity+WebView 중심; SYSTEM_ALERT_WINDOW 보유 앱의 FGS 시작 규칙(Android 12+) — Service 주도 오버레이 구조는 네이티브 필수.

## Decisions (with rationale)
- D1: 코어 추출 레이아웃은 참고 저장소를 그대로 따른다(crates/hanbeon-core + crates/hanbeon-jni 워크스페이스 멤버화). 이유: 실기 검증된 코드의 최소 변경 이식이 최저 위험.
- D2: 안드로이드 껍데기는 참고의 Kotlin 파일 8종(~2000줄)을 이식해 출발점으로 삼는다. ControllerView 최신 커밋(압축형 재설계 2b1881b) 포함.
- D3: 포크1 해소(2026-08-24 사용자 응답): 이 워크스페이스는 Tauri 파이프라인으로 패키징 시도. 참고 저장소(orca/hanbeon 순수 Gradle)는 그대로 별도 트랙. "Tauri만"= Tauri 앱+플러그인(Kotlin) 한 APK — 순수 Tauri(코틀린 0줄)는 오버레이·접근성이 OS API라 불가능함을 사용자에게 고지.
- D4(채택 기본값): 범위 = 참고의 실기 검증 기능 집합(오버레이 4칸, 접근성 출력, USB 입력, 간격 적응, 실증 기록)과 동등 수준, 단계별 go/no-go 게이트 포함. 사용자 미확인 — 게이트에서 거부 가능.
- D5(채택 기본값): 데스크톱 cargo+bun 테스트 회귀 없음 유지, 안드로이드는 실기 수동 확인, 에뮬레이터 자동화 범위 밖.
- D6: 참고 브랜치의 PRD 5.5 실기 교훈(focusSearch 불가→트리 순회, 커서 직접 기억, 강조 창 선오리기, 활성 창이 우리면 스킵)은 이식 자산으로 재사용.

## Scope IN
- 플랫폼 무관 코어 추출 + 데스크톱 회귀 없음
- 안드로이드 APK 빌드 파이프라인 (NDK, gradle)
- 참고 구현 이식: 오버레이 4칸 컨트롤러, 접근성 출력, USB 스위치 입력, 실증 기록
- (포크 2 결과에 따라) 소리/설정 화면/앱별 칸 실행 중 무엇을 할지

## Scope OUT (Must NOT have)

## Open questions
- Q1(포크1): Tauri 안드로이드를 어디까지 쓰나 — (A) 안 씀(참고와 동일, 추천) (B) 설정 UI만 Tauri Activity + 컨트롤러는 네이티브 하이브리드 (C) 전면 Tauri 시도
- Q2(포크2): 첫 마일스톤 범위 — 참고가 실기 검증한 것까지만 이식 vs 남은 것(소리·설정화면·앱별칸 실행)까지
- Q3(테스트): 데스크톱 cargo+bun 테스트 유지, 안드로이드는 실기 수동 확인 전제 — 에뮬레이터 자동화는 범위 밖으로 두는지

## Approval gate
status: approved (2026-08-24, user "고")
approach: Tauri v2 안드로이드 패키징 — 환경세팅 → 최소 스파이크+분기 게이트(빌드가 아니라 실기 서비스 생명주기·브리지 입증; 실패 시 순수 Gradle 폴백도 완성품 기준 충족) → 코어 추출(데스크톱 프로덕션 빌드 회귀 게이트 포함) → 네이티브 계층(제스처 우선 클릭 계약) → 코어 브리지 → 프로비저닝 체크리스트. momus r1 blocker(T분기 성공기준이 G폴백과 모순) 수정 완료, r2 진행.
<!-- When exploration is exhausted and unknowns are answered, set status: awaiting-approval. -->
<!-- That durable record is the loop guard: on a later turn read it and resume at the gate instead of re-running exploration. -->
