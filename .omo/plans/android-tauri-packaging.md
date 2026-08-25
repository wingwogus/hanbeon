# android-tauri-packaging - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 이 앱을 Tauri v2 파이프라인으로 안드로이드에 패키징한다. 새 안드로이드 기기에 디버그 APK를 설치해, 스위치 누름 → 4칸 오버레이 컨트롤러 → 다른 앱 조작까지 한 줄이 연결되는 것을 목표로 한다. 데스크톱 앱은 그대로 유지되며, 두 플랫폼이 같은 Rust 코어(스캔·간격 적응·기록)를 쓰게 된다.

**Why this approach:** Tauri 패키징이 목표지만, 오버레이 창은 포그라운드 Service가 소유해야 한다는 OS 구조 제약(참고 프로젝트가 실기로 확인한 것) 때문에 빌드 성공만으로는 안 되고 실기에서 서비스 생명주기와 코어 연결을 입증하는 게이트를 먼저 통과하게 했다. 통과하면 Tauri 플러그인 경로, 실패하면 이미 실기 검증된 순수 Gradle 방식으로 완성품을 내는 두 분기 구조다. 어느 쪽이든 스캔·적응·기록 코어는 같은 Rust 코드를 쓴다.

**What it will NOT do:** Play 스토어 배포 준비는 하지 않는다. iOS나 태블릿 내장 스위치 제어 연동은 범위 밖이다. 참고 저장소(orca/hanbeon)의 코드와 브랜치는 건드리지 않는다.

**Effort:** Large
**Risk:** High - Tauri Activity + 네이티브 Service 공존 구조의 실기 미검증성(그래서 스파이크 게이트가 있다)
**Decisions to sanity-check:** ① 스파이크 게이트 실패 시에도 폴백(순수 Gradle)으로 안드로이드 완성품까지 간다 — 여기서 멈추지 않음 ② 첫 목표를 참고의 실기 검증 수준(소리·설정화면·앱별 칸 실행 제외, Uno/CDC 하드웨어만)으로 제한

Your next move: 계획 승인 후 /start-work android-tauri-packaging으로 실행을 시작한다. Full execution detail follows below.

---

> TL;DR (machine): Large/High-risk — 6 todos + 4 final verifiers; Tauri v2 Android packaging with native plugin ports from verified reference, core crate extraction, on-device E2E gate

## Scope
### Must have
- **분기 구조**: (T) Tauri 분기 — `tauri android init` 프로젝트에 네이티브 플러그인을 얹어 목표 달성. (G) 폴백 분기 — 참고와 같은 순수 Gradle+JNI로 동등 APK 달성. 스파이크 게이트(3a/3b)에서 어느 분기로 갈지 결정되며, 두 분기 모두 완전한 성공 기준과 종단 검증을 가진다.
- 플랫폼 무관 코어를 `crates/hanbeon-core` 워크스페이스 멤버로 추출하고, 데스크톱(`apps/desktop/src-tauri`)이 path 의존으로 재사용한다.
- **데스크톱 회귀 게이트**: 코어 추출 후 `bun run desktop:build`(프로덕션 빌드) 1회 성공 + 기존 테스트 전부 통과. 최종 검증에서 1회 더.
- 오버레이 컨트롤러(4칸+Enter 칸), 접근성 출력, USB 시리얼 스위치 입력을 선택된 분기의 구조(T=플러그인 Kotlin / G=참고 Gradle 앱)로 구현해 같은 APK에 싣는다.
- **선택 동작은 참고 계약 그대로**: 제스처 탭 우선 + 실패 시 ACTION_CLICK 폴백(HanbeonAccessibilityService.kt:95-160), accessibility service XML에 canPerformGestures 등 참고 capability 세트 이식.
- 참고 브랜치(/Users/wingwogus/orca/hanbeon feat/android-one-row-controller)의 검증된 Kotlin 코드와 PRD 5.5 실기 교훈(focusSearch 불가→노드 트리 순회, 커서 위치는 코어가 기억, 강조 창 선오리기, 활성 창이 우리 자신이면 스킵)을 이식한다.
- 실증 기록(journal)이 데스크톱과 같은 JSON Lines 형식으로 앱 전용 폴더에 남고, **실기에서 생성된 파일**을 `adb shell run-as … cat`으로 덤프해 `bun run summary`가 파싱한다(샘플 fixture는 단위테스트까지만).
- 간격 적응 변경 시 이유 화면 표시(원칙 2) 유지.
- **안드로이드 앱별 칸(프리셋)**: 이번 반복에서는 안드로이드 쪽 확장 칸 선택·단축키 실행을 비활성화하고 기본 4칸만 유지(참고와 동일). 데스크톱의 Hana Cloud 레지스트리는 그대로 유지. 추후 별도 기능으로 활성화 가능.
- **플랫폼 하한선**(참고 baseline, 생성 프로젝트가 더 엄격하지 않는 한): minSdk 26, arm64-v8a 전용, USB Host 필수, foregroundServiceType="specialUse"+알림 권한 포함 매니페스트 선언, 접근성 XML에 canRetrieveWindowContent·제스처 capability.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 참고 저장소(orca/hanbeon 순수 Gradle 트랙)의 코드나 브랜치를 수정하지 않는다 — 읽기 전용 참고자료.
- 앞 4칸의 순서·자리 변경, 실행 중 스캔 대상 교체 금지(CLAUDE.md 하지 말 것).
- iOS/태블릿 스위치 제어 연동, 앱 전환 기능, 문자 입력 — 범위 밖.
- 에뮬레이터 UI 자동화 프레임워크 도입 금지(실기 수동 확인이 안드로이드 QA 표준).
- Play 스토어 서명·배포 준비 금지 — 디버그/사이드로드 APK까지만.
- CH340 클론 보드, non-CDC 시리얼, HID 전용, BT 스위치 지원 금지 — 검증된 Uno/CDC `P`/`R` 프로토콜만.
- 실측 없는 정량 지표 달성 주장 금지.

## Verification strategy
> 대부분의 검증은 에이전트 실행. 단 두 군데 인간 협력이 구조적으로 필요하며 계획이 이를 명시한다: (1) Android 13+ 사이드로드 앱의 "제한된 설정 허용"+오버레이·접근성 권한 최초 부여(기기 프로비저닝 전제조건), (2) 하드웨어 스위치 종단 확인. 프로비저닝 이후의 adb 기반 검사는 전부 에이전트 실행이다.
- Test decision: tests-after + 회귀 게이트. Rust: `cargo test --workspace`, 데스크톱: `bun run lint && bun run test` + `bun run desktop:build`(추출 직후·최종 각 1회). 안드로이드: 정확한 명령은 생성 프로젝트 경로 기준 `./gradlew assembleDebug` + `./gradlew testDebugUnitTest`, adb 단정(assert): `adb shell am start`, `pidof`, `dumpsys activity services`, logcat 패키지 필터.
- Evidence: `.omo/evidence/task-N-*.txt`
- **수동 전제조건 체크리스트**(todo 6 문서에 포함): 설치 → 제한된 설정 허용(Android 13+) → 오버레이 권한 → 접근성 서비스 ON. 활성화 후 강제종료/재설치 시 접근성이 꺼짐을 문서화.

## Execution strategy
### Parallel execution waves
- Wave 1 (병렬): 환경 세팅 / **최소 Tauri 안드로이드 스파이크(코어 추출 전)**
- Wave 2 (게이트): 스파이크 결과로 분기 결정 — (T) 플러그인 경로 vs (G) 순수 Gradle 폴백. **게이트는 빌드 성공이 아니라 실기 서비스 생명주기+브리지 입증**
- Wave 3: 코어 추출(hanbeon-core) → 데스크톱 프로덕션 빌드 회귀 확인
- Wave 4: 선택된 분기로 네이티브 기능 구현(오버레이·접근성 출력·USB 입력)
- Wave 5: 코어↔호스트 연결(브리지 계약: Service가 스캐너 기동, USB 입력→스캐너, Notice→오버레이 UI, Host.inject→접근성, Activity 백그라운드 시 동작 유지)
- Wave 6: 실기 종단 검증 준비 + 프로비저닝 체크리스트 + 문서화

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 환경 세팅 | - | 2(spike) | - |
| 2 최소 스파이크+분기 결정 | 1 | 3,4,5 | - |
| 3 코어 추출 | 1 | 4,5 | (2 완료 후) |
| 4 네이티브 기능 구현 | 2,3 | 6 | - |
| 5 코어 연결 | 3,4 | 6 | - |
| 6 종단 검증 준비 | 4,5 | F1-F4 | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. 안드로이드 빌드 환경 세팅 + NDK 경로 gitignore
  What to do / Must NOT do: Android NDK 설치(`sdkmanager --install ndk;26.3.11579264` 또는 실패 시 https://dl.google.com/android/repository 직접 다운로드), `rustup target add aarch64-linux-android`, `cargo install tauri-cli --version "^2"` 확인(이미 있으면 스킵). `.cargo/config.toml`에 aarch64-linux-android 링커/ar 경로 작성(NDK 설치 경로 기준, 머신 의존). **`.gitignore`에 `.cargo/config.toml` 추가 후 `git check-ignore -q .cargo/config.toml`로 단정.** Must NOT do: 기존 데스크톱 빌드 설정 변경 금지.
  Parallelization: Wave 1 | Blocked by: 없음 | Blocks: 2
  References: /Users/wingwogus/orca/hanbeon/apps/android/README.md(NDK 링커 설정 예시·sdkmanager 실패 시 우회), /Users/wingwogus/orca/hanbeon/scripts/android/build-core.sh:10-18(머신 의존 파일 취급 선례), ~/Library/Android/sdk(기존 SDK 위치), .gitignore(현재 .cargo 항목 없음)
  Acceptance criteria (agent-executable): `rustup target list --installed`에 aarch64-linux-android 포함; `$ANDROID_HOME/ndk/` 디렉터리 존재; `git check-ignore -q .cargo/config.toml && echo ignored`가 ignored 출력.
  QA scenarios: happy — 위 명령 전부 성공, Evidence .omo/evidence/task-1-env.txt / failure — sdkmanager 다운로드 실패 시 직접 다운로드 우회 후 재시도 로그, Evidence 동일 파일.
  Commit: Y (.gitignore만) | chore: ignore machine-local cargo android linker config
  Recommended task executor category: quick
- [ ] 2. 최소 Tauri 안드로이드 스파이크 + 분기 결정 게이트 (코어 추출보다 먼저)
  What to do / Must NOT do: `bun run -F ./apps/desktop tauri android init`(코어 추출 없이 현 데스크톱 crate 그대로 — Android 타깃 컴파일이 막히면 그것 자체가 게이트 증거) → `tauri android build --debug`. **게이트 판정은 빌드 성공이 아니라 다음의 실기 입증이다**: 기기에서 Tauri Activity 기동 → 네이티브 포그라운드 Service 시작·Activity 백그라운드 후에도 생존 → 오버레이 권한 부여 상태에서 TYPE_APPLICATION_OVERLAY 창 표시 → 테스트용 네이티브 호출이 Rust 스캐너(또는 stub Rust 함수)에 도달하고 접근성 동작 1회 시도까지 연결 → 접근성 서비스 비활성 시 조용한 성공이 아니라 식별 가능한 상태/로그. adb 증거 수집(`am start`, `pidof`, `dumpsys activity services`, logcat 필터). 판정 결과를 (T) 플러그인 경로 / (G) 순수 Gradle+JNI 폴백으로 기록하고 폴백 선택 시 참고 저장소 구조(apps/android+crates/hanbeon-jni)를 이식 계획으로 승격. gen 디렉터리 커밋 여부 결정해 .gitignore 정리. Must NOT do: 이 단계에서 제품 네이티브 기능 구현 금지(stub 검증까지만), 데스크톱 windows 설정 파손 금지.
  Parallelization: Wave 1-2 | Blocked by: 1 | Blocks: 3,4,5,6
  References: https://v2.tauri.app/develop/android/ , https://v2.tauri.app/develop/plugins/ , /Users/wingwogus/orca/hanbeon/apps/android/README.md:9-18(Service 소유 오버레이 제약 — 게이트의 근거), apps/desktop/src-tauri/tauri.conf.json, apps/desktop/src-tauri/Cargo.toml(macOS 전용 feature/의존 — Android 크로스 컴파일 방해 여부 확인 대상)
  Acceptance criteria (agent-executable): **분기별 성공 기준 — T 분기**: `tauri android build --debug` 종료 코드 0 + APK 경로 기록, 기기 연결 시 `adb install -r <apk>` 성공, 게이트 체크리스트(Service 생명주기·오버레이 표시·stub→Rust 도달·접근성 실패 식별) 각 항목의 adb/logcat 증거가 Evidence에 항목별로 존재. **G 분기**: 빌드/설치/서비스 입증 실패의 원인이 아키텍처적(Tauri 구조상 Service 브리지 불가)임을 Evidence에 기록하면 그것으로 게이트 충족 — G 전환 후 todo 4-6의 인수기준은 Gradle 등가물(참고 gradle 구조 APK, jniLibs .so, 참고 JNI 계약)로 대체 실행한다. 어느 쪽이든 **분기 결정(T/G)과 그 이유가 Evidence 말머리에 한 줄로 기록됨**.
  QA scenarios: happy — 게이트 전항 통과→T 기록 / failure — Service 생명주기 또는 브리지 입증 실패→G 전환 기록+폴백 이식 계획 명시, 둘 다 Evidence .omo/evidence/task-2-spike-gate.txt.
  Commit: Y | chore(android): tauri android spike and branch gate
  Recommended task executor category: unspecified-high
- [ ] 3. 플랫폼 무관 코어를 crates/hanbeon-core로 추출 + 데스크톱 프로덕션 빌드 회귀 게이트
  What to do / Must NOT do: 참고 저장소 /Users/wingwogus/orca/hanbeon/crates/hanbeon-core/src/(action, adapt, cue, gesture, host, journal, key, preset, profile, scan, shortcut, lib)를 현재 워크스페이스 crates/hanbeon-core로 가져오되, 현재 앱의 최신 기능(Hana Cloud 앱 프리셋 app_registry 등)과 불일치점을 현재 apps/desktop/src-tauri/src/ 코드와 대조해 흡수한다. 루트 Cargo.toml workspace members에 crates/hanbeon-core 추가, src-tauri는 path 의존으로 전환. tauri AppHandle 의존(scan.rs emit, profile.rs 저장 경로, journal.rs 열기)은 참고의 Host 트레이트(host.rs:42)+Notice 패턴으로 경계 분리. Must NOT do: 데스크톱 동작·UI 변경 금지(순수 리팩터링), 코어에 tauri 타입 노출 금지.
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 4,5
  References: apps/desktop/src-tauri/src/lib.rs(모듈 구성), scan.rs:26+584(AppHandle 결합 지점), profile.rs:122-151, journal.rs:108-185, journal.rs:32-92(이벤트 이름·타임스탬프 형식 — 안드로이드 호환 대상); /Users/wingwogus/orca/hanbeon/crates/hanbeon-core/src/host.rs(Host 트레이트 전문), /Users/wingwogus/orca/hanbeon/apps/desktop/src-tauri/src/lib.rs:17-29(데스크톱이 코어 크레이트 쓰는 선례), apps/desktop/src-tauri/Cargo.toml:14-43(플랫폼별 의존 보호 대상)
  Acceptance criteria (agent-executable): `cargo test --workspace` 전부 통과; `bun run lint` 통과; `grep -rn "use tauri" crates/hanbeon-core/src/` 0건; **`bun run desktop:build` 1회 성공**(산출물 경로 Evidence 기록).
  QA scenarios: happy — 위 4명령 성공, Evidence .omo/evidence/task-3-extract.txt / failure — 추출 중 데스크톱 테스트·프로덕션 빌드 깨지면 수정 전까지 진행 금지, 실패 로그 보존, Evidence 동일.
  Commit: Y | refactor(core): extract platform-free core crate
  Recommended task executor category: deep (1293줄 스캔 모듈의 결합 분리라 공유 통찰이 하나라 분할 불가)
- [ ] 4. 네이티브 기능 구현 — 오버레이·접근성 출력·USB 입력 (게이트 결과에 따른 구조로)
  What to do / Must NOT do: todo 2의 분기 결정을 따른다. (T) Tauri 플러그인(Kotlin)으로 / (G) 참고 Gradle 앱 구조로. 내용은 동일: (a) OverlayService — foregroundServiceType="specialUse"+알림 권한 포함 선언, TYPE_APPLICATION_OVERLAY+FLAG_NOT_FOCUSABLE 창에 4칸+Enter 칸 컨트롤러 렌더(참고 ControllerView.kt 2b1881b 압축형 이식); (b) HanbeonAccessibilityService — 포커스 이동(노드 트리 순회, focusSearch 금지), **선택은 제스처 탭 우선 + ACTION_CLICK 폴백**(참고 HanbeonAccessibilityService.kt:95-160 계약 그대로; service XML에 canPerformGestures·canRetrieveWindowContent 등 참고 capability 세트, res/xml/accessibility_service.xml:5-16), performGlobalAction(BACK), TYPE_WINDOW_STATE_CHANGED 포그라운드 감지; (c) UsbSwitch — USB Host API CDC `P`/`R` 수신(참고 UsbSwitch.kt 이식, CH340/non-CDC 배제). Manifest: SYSTEM_ALERT_WINDOW, FOREGROUND_SERVICE(+SPECIAL_USE), 알림, USB Host(feature required), 접근성 바인딩. minSdk 26·arm64-v8a 전용 baseline 유지(생성 프로젝트가 더 엄격하지 않는 한). Must NOT do: 칸 순서·자리 변경 금지, 안드로이드 확장 칸(앱별 프리셋 실행) 활성화 금지 — 기본 4칸만.
  Parallelization: Wave 4 | Blocked by: 2,3 | Blocks: 5,6
  References: /Users/wingwogus/orca/hanbeon/apps/android/app/src/main/java/kr/devfive/hanbeon/{ControllerView,OverlayService,HanbeonAccessibilityService,HighlightView,UsbSwitch,MainActivity}.kt(~2000줄 이식 출발점), /Users/wingwogus/orca/hanbeon/apps/android/app/src/main/{AndroidManifest.xml,res/xml/accessibility_service.xml}, /Users/wingwogus/orca/hanbeon/apps/android/app/build.gradle.kts(minSdk·abiFilters baseline), docs/PRD.md 5.5절 실기 교훈
  Acceptance criteria (agent-executable): 생성 프로젝트 경로에서 `./gradlew assembleDebug` 통과 + APK 경로 기록; `./gradlew testDebugUnitTest` 통과; Manifest/service XML 선언이 위 목록과 일치(grep 단정); 제스처 우선 클릭 계약이 코드에 존재(gesture dispatch 호출이 ACTION_CLICK 폴백보다 선행하는 구조 단정).
  QA scenarios: happy — 빌드·단위테스트·선언 단정 전부 통과, Evidence .omo/evidence/task-4-native.txt / failure — 오버레이 권한 미허가 기동 시 Service가 조용히 죽지 않고 식별 가능 상태/로그 남기는지 adb 재현(logcat 필터), Evidence 동일.
  Commit: Y | feat(android): native overlay/accessibility/usb host layer
  Recommended task executor category: unspecified-high
- [ ] 5. 코어와 안드로이드 호스트 연결 — 브리지 계약 구현 + 실증 기록
  What to do / Must NOT do: **브리지 계약을 todo 2 게이트에서 입증된 구조로 구현한다**: Service가 스캐너 기동/정지, USB press/release→스캐너, 코어 Notice→오버레이 UI, Host.inject→접근성 서비스, Tauri Activity 백그라운드/소멸 후에도 스캔 지속. (T) 분기라면 Tauri command/emit과 Service 사이 경로를, (G) 분기라면 참고 hanbeon-jni JNI 계약(Core.kt Callbacks 목록)을 따른다. 실증 기록(journal)은 앱 전용 files/logs에 JSON Lines, 문자열·숫자만 기록(타입 금지 규칙), 외부 전송 코드 금지. 간격 적응 변경 시 이유 emit(원칙 2). Must NOT do: 코어 로직 복제 금지.
  Parallelization: Wave 5 | Blocked by: 3,4 | Blocks: 6
  References: /Users/wingwogus/orca/hanbeon/crates/hanbeon-jni/src/lib.rs(AndroidHost — G분기 직접 사용/T분기 역할 대응 참조), /Users/wingwogus/orca/hanbeon/apps/android/app/src/main/java/kr/devfive/hanbeon/{Core,OverlayService}.kt(Service 주도 스캐너 기동 선례 :36-42,:149-154), crates/hanbeon-core/src/host.rs, apps/desktop/src-tauri/src/journal.rs(형식 준수 대상), /Users/wingwogus/orca/hanbeon/apps/android/README.md:94-104(run-as 저널 덤프 절차)
  Acceptance criteria (agent-executable): `cargo test --workspace` 통과; (T) `unzip -l <apk> | grep libhanbeon`로 .so 포함 단정 / (G) build-core.sh 산출 .so가 jniLibs 도달 단정; **실기 프로비저닝 후 `adb shell run-as kr.devfive.hanbeon cat files/logs/<당일>.jsonl` 덤프 → `bun run summary <덤프>`가 빈 세션이 아닌 리포트 출력**.
  QA scenarios: happy — run-as 덤프+summary 리포트, Evidence .omo/evidence/task-5-wiring.txt / failure — Notice→직렬화 누락 회귀 단위테스트(코어 Notice→JSON), Evidence 동일.
  Commit: Y | feat(android): wire core scanner to android host bridge
  Recommended task executor category: unspecified-high
- [ ] 6. 실기 종단 검증 준비 + 프로비저닝 체크리스트 + 개발자 문서
  What to do / Must NOT do: 검증 절차를 두 층으로 문서화한다. **수동 전제조건**(사람 필수): 설치 → 제한된 설정 허용(Android 13+) → 오버레이 권한 → 접근성 서비스 ON, 강제종료/재설치 시 반복 필요함 명시. **adb 자동 단계**(프로비저닝 후 에이전트 실행): am start, pidof/dumpsys 서비스 생존 단정, logcat 권한실패 감지, run-as 저널 덤프+summary. scripts/android/(빌드·check 스크립트)를 이 워크스페이스 구조에 맞게 둔다. 하드웨어 스위치 종단은 사용자 협력 단계로 명시. Must NOT do: 실측 없이 정량 지표 달성 주장 금지, UI 스크립팅 자동화 시도 금지(권한 리셋 위험).
  Parallelization: Wave 6 | Blocked by: 4,5 | Blocks: F1-F4
  References: /Users/wingwogus/orca/hanbeon/apps/android/README.md(전체 — 특히 :40-41 제한된 설정, :74-82 접근성 해제 함정, :94-104 run-as), /Users/wingwogus/orca/hanbeon/scripts/android/check.sh, apps/admin(summary CLI), docs/PRD.md 10.1
  Acceptance criteria (agent-executable): check 스크립트 dry-run(기기 미연결)이 안내 메시지+정상 종료 코드; 체크리스트 문서에 수동/자동 층 구분 존재;
  QA scenarios: happy — dry-run 로그+문서, Evidence .omo/evidence/task-6-docs.txt / failure — 미연결 시 정상 종료 코드 확인, Evidence 동일.
  Commit: Y | docs(android): provisioning checklist and verification scripts
  Recommended task executor category: quick

> 브랜치: 현재 feat/android-tauri에서 계속한다. 모든 커밋은 이 브랜치 위에 쌓인다.

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — 계획의 각 todo 인수기준을 실제 산출물·Evidence와 대조: `cargo test --workspace`, `bun run lint`, `bun run desktop:build` 재실행, Evidence 파일 6건 존재 확인. 결과 .omo/evidence/f1-audit.txt
- [ ] F2. Code quality review — crates/hanbeon-core에 tauri 의존 0건 재단정, 코어 경계(Host 트레이트) 위반 여부, clippy -D warnings 통과. 결과 .omo/evidence/f2-quality.txt
- [ ] F3. Real manual QA — 프로비저닝된 기기에서 adb 자동 단계 전체 재실행(서비스 생존, 오버레이 표시, run-as 저널→summary). 하드웨어 스위치 종단은 사용자 협력 항목으로 명시적 미완료 표기 허용. 결과 .omo/evidence/f3-manual.txt
- [ ] F4. Scope fidelity — Must NOT have 목록 위반 검사(참고 저장소 변경 없음 git status, 칸 구조 무변경, CH340 등 배제 유지), 분기 게이트 기록(T/G)과 실제 구조 일치 확인. 결과 .omo/evidence/f4-scope.txt

## Commit strategy
- todo 1(.gitignore), 2(spike+게이트), 3(refactor core), 4(native layer), 5(bridge), 6(docs) 각각 커밋 1개. husky pre-commit(`bun run lint`)이 각 커밋에서 회귀 감지.
- husky pre-commit(`bun run lint`)가 각 커밋에서 돈다 — cargo clippy/fmt/check 포함이라 코어 추출 커밋에서 자동 회귀 감지된다.
- 브랜치: 현재 feat/android-tauri에서 계속한다.

## Success criteria
1. `cargo test --workspace` + `bun run lint` 통과(데스크톱 회귀 없음).
2. **분기별**: (T) `tauri android build --debug` 산출 APK 설치 가능 / (G) 폴백 Gradle 빌드 산출 APK 설치 가능 — 어느 쪽이든 설치 가능한 APK가 존재.
3. 코어 추출 후 crates/hanbeon-core에 tauri 의존 0건.
4. 실증 기록이 `bun run summary`로 파싱됨.
5. (사용자 협력) 새 안드로이드 기기에서 스위치→오버레이→타앱 조작 종단 동작 확인 — 에이전트 단독 검증 불가 항목은 체크리스트로 남기고 미달성 주장 금지.
