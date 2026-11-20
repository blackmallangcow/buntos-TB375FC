# 보안 정책

> **English summary:** Please do not report vulnerabilities in public issues or pull requests. Use the "Report a vulnerability" button on the repository's Security tab (GitHub private vulnerability reporting). Vulnerabilities in upstream Linux, Debian, or other third-party projects should go to those projects.

## 취약점 제보 방법

취약점은 공개 이슈나 PR로 올리지 말아 주세요. 고쳐지기 전에 공개되면 다른 사용자가 위험해집니다.

1. 이 저장소의 **Security** 탭을 엽니다.
2. **Report a vulnerability**를 눌러 비공개로 제보합니다.

제보에는 다음을 적어 주세요.

- 어떤 컴포넌트인지 (컴포지터, 셸, 에이전트, 이미지, BSP 등)
- 재현 방법과 영향 (무엇을 할 수 있게 되는지)
- 확인한 커밋이나 이미지 버전
- 알고 있다면 고치는 방법

한국어와 영어 모두 괜찮습니다.

## 대상 범위

이 저장소가 직접 만드는 코드가 대상입니다.

- buntOS 컴포지터, 셸, 에이전트, 설정·잠금·로그인 화면 등 데스크톱 컴포넌트
- 이미지 빌드와 BSP, 이 저장소에 있는 서비스와 설정
- 이 저장소의 커널 패치와 DTS 중 우리가 새로 쓴 부분

다음은 각 프로젝트에 제보해 주세요. 이 저장소에서는 고칠 수 없습니다.

- 업스트림 Linux 커널, Debian 패키지, 그 밖의 서드파티 소프트웨어의 취약점
- 재배포할 수 없어 이 저장소에 넣지 않은 제조사 펌웨어의 취약점

업스트림 문제에 우리 패치나 설정이 영향을 준다면 알려 주세요. 업스트림에도 함께 전달할지 같이 정하겠습니다.

## 지원 버전

별도 릴리스 없이 `main` 브랜치를 유지하므로, 보안 수정은 `main`에만 들어갑니다.

## 제보를 받은 뒤

제보는 프로젝트 관리자가 확인하고, 같은 비공개 제보 화면에서 답합니다. 고침이 나오기 전에는 내용을 공개하지 않고, 공개 시점은 제보자와 함께 정합니다. 제보자가 원하면 공개 글에 이름을 적습니다.
