# TB375FC 문서

이 저장소(`buntos-TB375FC`)는 buntOS 태블릿 라인의 첫 포크로, Lenovo Xiaoxin Pad Pro 12.7 (2025, **TB375FC**)에 올린다. 글로벌판 Lenovo Idea Tab Pro(TB373FU)는 같은 하드웨어라 같이 지원한다. buntOS 원본은 x86-64·arm64 데스크톱 라인이고, 태블릿은 원본의 arm64를 포크해 기기마다 최적화한다(buntOS 결정 자료 ADR-013, 2026-11-01).
- 데스크톱(컴포지터, 셸, 시스템 설정, 로그인 화면, 에이전트)은 원본에서 받아 합친다. 공용 고침은 원본에 먼저 낸다.
- 이 포크에서 더하는 것은 기기 쪽이다: 커널 커스텀(mainline + MT6897 포팅), 장치 트리, 부트 이미지와 언락, 기기 맞춤 최적화.
- 원본 buntOS는 원격 `buntos`(로컬 `../buntOS`)로 두고 필요할 때 받아 합친다.
- arm64만 빌드한다. x86-64 이미지와 CI는 원본의 몫이다.

## 문서

| 문서 | 내용 |
|---|---|
| [DESIGN.md](DESIGN.md) | **최종 설계안.** 핵심 결정, 부팅 길, 단계별 bring-up, 블록별 포팅 계획, 기기 맞춤 최적화, 위험, 일정 |
| [bringup-checklist.md](bringup-checklist.md) | **기기에서 할 일의 차례.** 백업, 언락, 2a·2b·2c·USB 호스트의 확인 항목과 명령, 위험과 되돌리는 길 |
| [lk-kernel-tree.md](lk-kernel-tree.md) | 순정 LK가 커널 장치 트리에 요구하는 것(코드로 읽음, 기기 미확인): 시험 커널을 그만두고 LK가 우리 트리를 넘기게 하는 길 |
| [porting-materials.md](porting-materials.md) | 포팅 재료 조사: 블록마다 가까운 업스트림 드라이버, 공개 벤더 소스, 포팅 크기, 업스트림 판, 부트 체인과 복구 |
| [wifi-soc7_0.md](wifi-soc7_0.md) | Wi-Fi·Bluetooth(connsys soc7_0, A-die MT6637) 단서 확인, LK 적재, MCU 명령 비교, 포팅 설계 |
| [community-port.md](community-port.md) | 이 기기의 커뮤니티 mainline 포트 분석: 커밋, 우리 계획과 겹침, 부팅과 기록, 협력할 일 |
| [../hw/lenovo-tb375fc.md](../hw/lenovo-tb375fc.md) | 펌웨어에서 읽은 하드웨어 실측(장치 트리, 부트 이미지, 커널), 중국판·글로벌판 비교 |
| [../design/ARCHITECTURE.md](../design/ARCHITECTURE.md) | buntOS 설계안(4.2절 실기기, 12장 Phase 0 실측 항목) |

## 기기 쪽 코드

| 곳 | 내용 |
|---|---|
| `kernel/tb375fc/` | 커널 커스텀: Linux 7.2.x(buntOS 이미지의 Debian forky와 같은 판)에 얹는 설정 조각(`config/`), 패치 대기열(`patches/`), 장치 트리(`dts/`), 빌드 스크립트 |
| `bsp/lenovo-tb375fc/` | 부트 이미지(boot·vendor_boot·init_boot·dtbo)와 initramfs 만들기 |
| `tools/hw/` | 펌웨어를 받고 푸는 도구(원격 ZIP 부분 받기, 부트 이미지 풀기, 켜진 노드 뽑기) |

## 빌드 VM

- buntOS와 같은 Lima VM `buntos-dev`를 쓴다. VM에 이 포크도 마운트되어 있어야 한다(2026-11-01부터, 같은 경로, 쓰기 가능).
- 새로 만들 때는 저장소 맨 위 폴더에서 `limactl edit buntos-dev --mount "$PWD:w" --tty=false`로 더하고 VM을 다시 켠다.
- 커널 빌드 도구는 `kernel/tb375fc/README.md`에 있다.

## 규칙

- 펌웨어 파일(레노버·MediaTek 저작물)과 펌웨어에서 푼 벤더 장치 트리는 저장소에 넣지 않는다. 읽어 낸 사실과 우리 코드만 둔다.
- 벤더 커널 소스(OPPO, OnePlus, Motorola, GPL)는 참고만 한다. 옮겨 쓰는 코드는 출처와 원래 저작권 줄을 남기고 업스트림 꼴로 다시 쓴다.
- 실기기 작업은 buntOS 설계안 4.2.3절의 안전 수칙(전체 파티션 백업, 언락 진행 조건)을 따른다. 언락 진행 조건 넷은 길이 보이지만 모두 기기에서 확인한다([DESIGN.md](DESIGN.md) 2.1절).
- 조사는 공신력 있는 출처만 쓴다: 업스트림 커널, AOSP, OEM 공식 저장소, 이름난 오픈소스 프로젝트. 이 기기의 개인 저장소와 제3자 미러는 그렇다고 밝히고 쓰며, 블로그와 뉴스는 쓰지 않는다.
