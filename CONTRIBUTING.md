# 기여 안내

## DCO 서명

buntOS는 기여 조건으로 [Developer Certificate of Origin](https://developercertificate.org/)(DCO)을 씁니다. 리눅스 커널과 같은 방식입니다. 모든 커밋 메시지 끝에 다음 줄을 붙여 주세요.

```
Signed-off-by: 이름 <이메일>
```

`git commit -s`를 쓰면 자동으로 붙습니다. 이 줄은 DCO의 내용에 동의하며, 해당 기여를 이 저장소의 라이선스로 제출할 권리가 있다는 뜻입니다.

## 라이선스 표기

새 파일에는 SPDX 헤더를 답니다. 헤더를 달 수 없는 파일은 `REUSE.toml`에 표기합니다.

| 대상 | `SPDX-License-Identifier` |
|---|---|
| 핵심 컴포넌트(컴포지터, 셸, 에이전트) | `GPL-3.0-or-later` |
| 외부 앱이 링크하는 SDK·프로토콜 크레이트 | `MIT OR Apache-2.0` |
| 문서 | `CC-BY-SA-4.0` |

Rust 파일의 예:

```rust
// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

- 어떤 라이선스를 처음 쓰게 되면 원문을 `LICENSES/`에 추가합니다 (`reuse download <SPDX-ID>`)
- 제출 전에 `reuse lint`로 확인합니다. CI도 같은 검사를 합니다

## 커널과 DTS

커널 패치와 DTS는 업스트림(linux-mediatek 등) 제출을 기본으로 하며, 업스트림 규칙(커널 코드는 GPL-2.0, DTS는 이중 라이선스 관례)을 따릅니다. 자세한 내용은 [설계안](docs/design/ARCHITECTURE.md) 4.2.2를 참고하세요.

## 펌웨어

재배포할 수 없는 펌웨어는 저장소와 이미지에 넣지 않습니다. 설계안 2.1절의 규칙을 따릅니다.

## 설계 문서

설계 문서는 현재 한국어로 작성합니다. 설계를 바꾸면 `docs/design/`의 설계안과 결정 자료를 함께 갱신합니다.
