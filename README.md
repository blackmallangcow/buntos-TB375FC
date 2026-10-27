# buntOS

태블릿 폼팩터의 개발용 Unix 워크스테이션을 목표로 하는 오픈소스 OS입니다. 안드로이드 태블릿에서 막혀 있던 셸 제어, 개발환경 구성, 테스트 호스팅, 앱 간 제어를 완전히 열린 스택 위에서 제공하려고 합니다.

*English: buntOS is an open-source OS that aims to turn tablets into fully open Unix development workstations: Debian-based, mainline Linux, a Rust/Smithay Wayland compositor, and an AI-friendly control plane. The project is in the design phase, and the design documents are written in Korean.*

## 현재 상태

설계 단계입니다. 아직 코드와 빌드 이미지가 없습니다.

- 설계안: [docs/design/ARCHITECTURE.md](docs/design/ARCHITECTURE.md)
- 결정 자료와 근거: [docs/design/DECISION-BRIEF.md](docs/design/DECISION-BRIEF.md)

## 핵심 방향

- **완전히 열린 스택**: 메인 CPU에서 도는 커널·드라이버·사용자 공간은 전부 오픈소스입니다. 벤더 커널과 폐쇄 드라이버는 쓰지 않으며, 펌웨어만 예외입니다
- **개발환경 비차단**: root, 패키지 설치, 컨테이너, 가상화, 커널 업그레이드를 OS가 막지 않습니다
- **두 아키텍처 동등 지원**: x86-64와 arm64를 같은 소스와 같은 패키지 목록으로 빌드합니다
- **기반**: mainline Linux 커널, Debian(glibc, systemd), Rust + Smithay 기반 Wayland 컴포지터
- **AI 친화 제어 계층**: 접근성 트리를 바탕으로 권한을 확인하며 앱 간 제어를 제공합니다

첫 실기기 대상은 Lenovo Xiaoxin Pad Pro 12.7(TB375FC, MediaTek Dimensity 8300)이며, mainline 커널 포팅으로 진행합니다.

## 보안 지원

buntOS는 Debian forky(testing) 스냅샷을 기반으로 합니다. forky가 정식 릴리스되기 전의 빌드는 **보안 지원이 없습니다**. 실사용 환경에 쓰지 마세요.

## 라이선스

[REUSE](https://reuse.software/) 규격을 따릅니다. 파일별 라이선스는 SPDX 헤더나 `REUSE.toml`에 표기합니다.

| 대상 | 라이선스 |
|---|---|
| 컴포지터, 셸, 에이전트 등 핵심 컴포넌트 | GPL-3.0-or-later |
| 외부 앱이 링크하는 SDK·프로토콜 크레이트 | MIT OR Apache-2.0 |
| 문서 | CC-BY-SA-4.0 |

라이선스 원문은 [LICENSES/](LICENSES/)에 있습니다. 이 구성을 택한 이유는 결정 자료의 ADR-006에 있습니다.

## 기여

[CONTRIBUTING.md](CONTRIBUTING.md)를 참고하세요. 기여는 DCO(Developer Certificate of Origin) 서명으로 받습니다.
