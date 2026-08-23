# 한번 (HanBeon)

> 한 번의 누름으로 PC·태블릿을 제어하고, 피로와 반응속도 변화에 맞춰 주사 간격·피드백·오류 복구를 조정하는 상황적응형 싱글스위치

제7회 국립재활원 보조기기 해커톤 — 팀 **데브파이브**

- 제품 기획: [`docs/PRD.md`](docs/PRD.md)
- 개발 규약: [`CLAUDE.md`](CLAUDE.md)

## 구성

| 경로                     | 설명                                                            |
| ------------------------ | --------------------------------------------------------------- |
| `apps/desktop`           | Tauri 데스크톱 앱. 화면에 상시 떠 있는 floating 스위치 컨트롤러 |
| `apps/desktop/src-tauri` | Rust 코어. 스위치 입력 수신, 스캔 엔진, 전역 키 주입            |
| `apps/front`             | 랜딩·사용설명서 웹                                              |
| `apps/admin`             | 실증 로그·정량 지표 대시보드                                    |
| `apis/api`               | 사용자 프로필 동기화 API (Rust / vespera)                       |

## 사전 요구사항

```sh
# Bun
curl -fsSL https://bun.sh/install | bash

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Tauri CLI
cargo install tauri-cli --version "^2"
```

macOS는 Xcode Command Line Tools, Windows는 Microsoft C++ Build Tools와 WebView2가 필요합니다.

## 실행

```sh
bun install

bun run desktop        # Tauri 앱 개발 모드
bun run api            # 프로필 동기화 API
bun run dev            # front / admin 웹
```

### Arduino Uno 스위치

준비된 Uno를 쓰려면 **먼저 한번을 켠 뒤** USB로 꽂습니다. 앱이 보드를 찾고, 컨트롤러 맨 아래 줄에 연결 상태를 보여 줍니다. 뽑았다가 다시 꽂아도 앱을 다시 켤 필요는 없습니다.

```sh
bun run desktop
```

Python 브리지나 터미널 키 합성은 사용자 설정이 아닙니다. 개발 진단용 절차는 아래 개발용 환경변수와 브리지 README에 있습니다.

손쉬운 사용 권한은 **다른 앱에 키를 넣을 때**만 필요합니다. Uno를 찾고 버튼을 읽는 경로에는 필요하지 않습니다.

### macOS 접근성 권한

앱이 다른 프로그램에 키 입력을 보내려면 **시스템 설정 → 개인정보 보호 및 보안 → 손쉬운 사용**에서 권한을 허용해야 합니다. 개발 모드에서는 앱이 아니라 **`tauri dev`를 실행한 터미널(또는 IDE)** 에 권한을 줘야 합니다.

권한이 없으면 키 주입이 에러 없이 무시됩니다. 동작하지 않으면 코드를 의심하기 전에 권한부터 확인하세요.

### 개발용 환경변수

모두 개발·검증용 통로이고, 사용자용 설정 항목은 M3에서 프로필로 들어갑니다.

| 변수                  | 예           | 설명                                                                 |
| --------------------- | ------------ | -------------------------------------------------------------------- |
| `HANBEON_SWITCH_KEY`  | `F6`         | 스위치가 보내는 키. `KeyboardEvent.code` 표기. 기본 `F13`             |
| `HANBEON_INTERVAL_MS` | `6000`       | 시작 주사 간격. 타이밍에 의존하는 검증을 할 때 넉넉히 잡습니다        |
| `HANBEON_SOUND`       | `off`        | 청각 피드백 끄기                                                      |
| `HANBEON_LOG`         | `1`          | 프로필 로드, 스위치 입력, 상태 전이, 간격 조정을 stderr로 출력         |

MacBook 내장 키보드에는 `F13`이 없으므로 실기 스위치 없이 검증할 때는 키를 바꿔야 합니다.

```sh
HANBEON_SWITCH_KEY=F6 HANBEON_LOG=1 bun run desktop
```

### M1 검증 절차

이 프로젝트에서 가장 깨지기 쉬운 지점은 **floating 창이 대상 앱의 포커스를 뺏지 않는가**입니다. 창이 활성화되면 주입한 `Tab`이 대상 앱이 아니라 우리 창으로 들어가 제품 전체가 무의미해집니다.

1. 터미널에 손쉬운 사용 권한을 부여한다.
2. `HANBEON_SWITCH_KEY=<내장 키보드에 있는 키> bun run desktop`
3. 브라우저를 열고 링크가 여러 개인 페이지로 이동한다.
4. **브라우저를 클릭해 포커스를 준 뒤**, floating 창은 건드리지 않고 스위치 키만 누른다.
5. 확인할 것:
   - 커서가 `>`에 있을 때 누르면 브라우저의 포커스 링이 다음 요소로 이동하는가
   - 커서가 `<`에 있을 때 누르면 이전 요소로 돌아가는가
   - `Enter`에서 누르면 링크가 열리는가
   - 그 사이 브라우저가 계속 활성 상태인가 (창 제목 표시줄이 흐려지지 않는가)
   - 길게 누르면 일시정지되고, 다시 길게 누르면 재개되는가

`> `가 대상 앱이 아니라 한번 창 안에서 도는 것처럼 보이면 non-activating 처리가 실패한 것입니다. `src-tauri/src/window.rs`를 확인하세요.

### 사용자 프로필

설정은 아래 파일에 저장되며 앱을 켤 때 복원됩니다. 지우면 초기 설정 3단계 안내부터 다시 시작합니다.

```
macOS   ~/Library/Application Support/kr.devfive.hanbeon/profile.json
Windows %APPDATA%\kr.devfive.hanbeon\profile.json
```

적응 모드로 조정된 속도도 이 파일에 바로 기록됩니다. 강제 종료되더라도 사용자가 익숙해진 속도를 잃지 않아야 하기 때문입니다.

## 검사

```sh
bun run lint           # oxlint + clippy + cargo fmt
bun run test           # bun test + cargo tarpaulin
```

## 배포

기본적으로 docker compose로 작동되게 설계됨 (웹·API 한정). 데스크톱 앱은 `bun run desktop:build`로 플랫폼별 번들을 생성합니다.

배포는 CI에 두지 않습니다. `.github/workflows/ci.yml`은 lint·test·build 검사만 하며 GitHub 호스팅 러너에서 돕니다.

## 기여

이슈와 PR을 환영합니다. 다만 아래 두 가지는 코드 리뷰 이전의 전제입니다.

- **접근성 원칙이 기능보다 우선합니다.** [`CLAUDE.md`](CLAUDE.md)의 '접근성 원칙' 7개 항목은 협상 대상이 아닙니다. 이 소프트웨어의 사용자는 실수 한 번의 비용이 매우 큽니다.
- **스캔 대상 4칸 구조**는 제품의 핵심 가설입니다. 이 구조를 바꾸는 변경은 [`docs/PRD.md`](docs/PRD.md) 개정을 먼저 합의한 뒤에 진행합니다.

`bun run lint`와 `bun run test`가 통과해야 하며, lint는 husky pre-commit에서 자동으로 돕니다.

floating 창·키 주입과 관련된 변경은 **실제로 다른 앱에 포커스를 둔 채로** 검증한 결과를 PR에 적어주세요. 프론트만 보고 판단하면 반드시 놓칩니다.

## 보안 신고

이 앱은 전역 키 입력을 수신하고 다른 앱에 키를 주입하며, macOS 손쉬운 사용 권한을 사용합니다. 취약점을 발견하면 공개 이슈 대신 GitHub Security Advisory(`Security → Report a vulnerability`)로 알려주세요.

## 라이선스

[MIT](LICENSE).

번들로 포함한 제3자 자산(Pretendard 폰트, SIL Open Font License 1.1)은 [`NOTICE.md`](NOTICE.md)에 정리했습니다.
