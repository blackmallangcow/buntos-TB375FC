# buntOS

태블릿 폼팩터의 개발용 Unix 워크스테이션을 목표로 하는 오픈소스 OS입니다. 안드로이드 태블릿에서 막혀 있던 셸 제어, 개발환경 구성, 테스트 호스팅, 앱 간 제어를 완전히 열린 스택 위에서 제공하려고 합니다.

*English: buntOS is an open-source OS that aims to turn tablets into fully open Unix development workstations: Debian-based, mainline Linux, a Rust/Smithay Wayland compositor, and an AI-friendly control plane. The same image recipe builds and boots on x86-64 and arm64 QEMU with its own compositor, macOS-style shell, settings app and AI agent control plane (CLI, JavaScript REPL and MCP); the remaining foundation step is surveying the first target tablet. The design documents are written in Korean.*

## 현재 상태

x86-64와 arm64 QEMU 이미지를 같은 레시피로 빌드해 셸까지 부팅하며, 이미지에는 Wi-Fi·Bluetooth·오디오·GPU 펌웨어 같은 필수 드라이버가 들어 있습니다. 두 아키텍처 모두에서 e2e 시험(부팅과 로그인, AI 에이전트의 앱 간 제어, 키보드 단축키, 한글 입력, 화면 잠금)을 통과합니다(x86-64는 CI의 호스티드 러너에서). 기반 단계(Phase 0)의 남은 일은 첫 대상 기기의 실측입니다.

자체 컴포지터(bunt-comp), 셸(bunt-shell), 시스템 설정(bunt-settings), 로그인 화면(bunt-greeter)이 뜹니다. UI는 macOS 26을 기준으로 삼습니다([디자인 스펙](docs/design/DESIGN-SPEC.md)).
- **메뉴 막대:** 앞에 있는 앱의 메뉴, 입력 소스·사운드·배터리·Wi-Fi 같은 메뉴 막대 항목, 제어 센터, 알림 센터
- **Dock:** 최소화 칸과 최소화된 윈도우 애니메이션(지니 효과, 크기 조절 효과), 화면 가운데에 뜨는 모든 앱 패널, ⌘⇥ 앱 전환
- **데스크탑:** 데스크탑 폴더의 파일과 폴더(열기, 이름 변경, 휴지통으로 이동, 드래그해 옮기기와 다른 앱에 놓기), 빈 곳에서 드래그해 여러 항목 선택
- **터치:** 손가락 입력, 화면 가장자리에서 밀어 숨은 메뉴 막대·Dock 꺼내기, 키보드와 마우스가 없으면 누를 영역을 44px로 넓히는 터치 모드, 글 칸을 누르면 뜨는 iPadOS형 화면 키보드(2벌식 한글, 잠금·로그인 화면 포함)
- **스크린샷과 화면 기록:** ⇧⌘3·⇧⌘4, ⇧⌘5 도구 막대(선택 영역을 옮기고 크기를 조절해 찍기, 전체 화면·선택 부분 기록)
- **한글 입력:** fcitx5 두벌식. Ctrl+Space와 한/영 키로 바꾼다
- **로그인과 잠금:** 처음 켤 때 계정 만들기, 로그인 화면, 화면 잠금, 관리자 인증 창
- **시스템 설정:** Wi-Fi, Bluetooth, 디스플레이, 키보드, 사운드, 알림, 사용자 및 그룹, AI 에이전트 등 19개 페이지

리눅스에서 널리 쓰는 기본 앱(웹 브라우저, 파일, 텍스트 편집기, 계산기, 이미지 보기, 문서 보기, 동영상 재생, 시스템 모니터, 터미널)도 함께 들어 있습니다. Apple Silicon Mac에서는 이 이미지를 QEMU 창으로 띄워 볼 수 있습니다([개발 환경](docs/dev/environment.md) 4장).

**AI 제어 계층:** AI는 접근성 트리를 읽어 앱을 다룹니다. `bunt agent`(스냅샷과 동작), Playwright 문법의 JavaScript REPL(`bunt agent repl`), 셸이 없는 AI 앱을 위한 MCP 어댑터(`bunt mcp`)가 있습니다. 사용자가 시스템 설정에서 허용한 AI만 제어할 수 있고, 관리자 인증·암호 창은 사람만 답하며, 화면이 잠긴 동안에는 아무것도 하지 못합니다.

![buntOS 데스크톱: 메뉴 막대 오른쪽의 입력 소스(한)·사운드·전원·네트워크·제어 센터 아이콘, 흐린 유리 위에 열린 제어 센터, macOS 26형 시스템 설정의 키보드 페이지(입력 소스 ABC·한글, 전환 단축키), Dock](docs/images/desktop-v0.png)

- 설계안: [docs/design/ARCHITECTURE.md](docs/design/ARCHITECTURE.md)
- 결정 자료와 근거: [docs/design/DECISION-BRIEF.md](docs/design/DECISION-BRIEF.md)
- 디자인 스펙(UI 규칙과 토큰): [docs/design/DESIGN-SPEC.md](docs/design/DESIGN-SPEC.md)
- 개발 환경과 이미지 빌드: [docs/dev/environment.md](docs/dev/environment.md)

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
