# 제3자 구성요소 고지

이 저장소는 '한번(HanBeon)' 자체 코드 외에 아래 구성요소를 함께 배포합니다.
각 구성요소는 원 라이선스를 따릅니다.

## 번들로 포함된 자산

| 자산       | 경로                                              | 라이선스                        |
| ---------- | ------------------------------------------------- | ------------------------------- |
| Pretendard | `apps/desktop/public/fonts/PretendardVariable.woff2` | SIL Open Font License 1.1       |

Pretendard © 길형진(orioncactus). 원문은 같은 폴더의 [`OFL.txt`](apps/desktop/public/fonts/OFL.txt)에 있습니다.

데스크톱 앱은 오프라인에서 동작해야 하므로 폰트를 CDN이 아니라 번들로 포함합니다.
OFL 1.1은 이런 재배포를 허용하며, 폰트 파일 자체를 판매하지 않고 라이선스 전문을
함께 배포할 것을 요구합니다. 두 조건 모두 충족합니다.

## 의존성

Rust(`Cargo.toml`)와 JavaScript(`package.json`) 의존성은 저장소에 포함하지 않고
빌드 시점에 내려받습니다. 2026-08 기준 Rust 의존성 670개에 GPL·AGPL 계열은 없으며
MIT / Apache-2.0 / BSD / Zlib / Unicode-3.0, 그리고 MPL-2.0 5개로 구성되어 있습니다.
MPL-2.0은 파일 단위 카피레프트라 이 프로젝트의 MIT 배포와 충돌하지 않습니다.

전체 목록은 아래로 확인할 수 있습니다.

```sh
cargo install cargo-license && cargo license
```
