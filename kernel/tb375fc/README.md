# TB375FC 커널

TB375FC(Lenovo Xiaoxin Pad Pro 12.7 2025, MediaTek MT6897)용 mainline 커널 커스텀이다. 설계는 [docs/tb375fc/DESIGN.md](../../docs/tb375fc/DESIGN.md)의 1절(D1, D2), 3절, 4절, 5절을 따른다.

- **기반:** Linux **7.2.12** 원본(kernel.org)이다. buntOS 이미지의 Debian forky 커널과 같은 판이다. 벤더 GKI 커널은 쓰지 않는다. 이미지용 커널도 이 원본 소스로 빌드하고, 설정만 Debian forky 설정에 조각을 얹은 것을 쓴다. Debian 소스 패키지의 패치는 쓰지 않는다(설계안 D1).
- **커스텀:** 원본 위에 세 가지를 얹는다. 모두 업스트림에 낼 수 있는 꼴로 짠다.
  - 장치 트리(`dts/`)
  - 패치 대기열(`patches/`)
  - 보드 설정 조각(`config/`)

| 곳 | 내용 |
|---|---|
| `build.sh` | 원본 받기(sha256 확인), 작업 트리에 패치·장치 트리 얹기, 설정 합치기, 빌드. 이미지용 커널 .deb(`image-config`·`image`) |
| `debian-config.sh` | 이미지용 설정. 이미지와 같은 시점의 Debian forky 커널 설정(서명·sha256 확인)에 조각을 얹고, 빠진 값을 알린다. `build.sh image-config`가 이미지용 작업 트리로 부른다(`KERNEL_TREE`·`KERNEL_OUT`·`STRICT=1`) |
| `config/tb375fc.config` | 보드 설정 조각. 단계(2a~2e, 3)별로 묶었다. 7.2.12에 있는 기호만 쓴다 |
| `patches/series` | 패치 대기열과 다음 차례. 0001 장치 트리 빌드 목록, 0002·0003 SPMI 업스트림 고침 백포트, 0004~0006 MT6897 핀 컨트롤러(바인딩, pinmux 매크로, 드라이버), 0007·0008 SPMI 고침(업스트림에 없음), 0009~0013 MT6897 클럭(바인딩, HW voter syscon, FENC 없는 HW voter 먹스, PLL HW voter, 드라이버), 0014~0016 MFD `mtk-spmi-pmic`(업스트림 v12 백포트와 고침), 0017·0018 MT6363 키, 0019~0022 MT6897 UFS(PHY·호스트 바인딩과 드라이버), 0023~0026 MT6368(regulator 바인딩·드라이버, MFD 데이터), 0027~0031 터치·펜(핀 drive-strength, pinmux 헤더 고침, SPI 바인딩의 MT6897, NT36532 터치 바인딩·드라이버), 0032~0036 포고 키보드(v6 백포트와 리뷰 지적, 레노버 꼴, UART 바인딩의 MT6897과 패드 모드, 8250_mtk 고침), 0037~0041 MT6363 regulator 고침(8비트 값 regmap, VEMC 전압, OC 처리, VSRAM 레지스터. 업스트림에 낼 것), 0042~0049 USB 호스트의 VBUS(I2C 바인딩의 MT6897, I2C 클럭 게이트, MT6375 MFD와 OTG 부스트, 차지 펌프 입력 FET 스위치. 모두 꺼 둔 노드), 0050~0054 MT6897 전원 도메인(바인딩 둘, probe 실패 때 기본 꺼짐 도메인을 끄지 않는 고침(업스트림에 올라옴), 한 제어기에 직접 제어와 HW voter, 표), 0080~0084 NT36532 패널(업스트림 드라이버와 바인딩 백포트, DSI 플래그, 이 기기의 패널 셋. 화면 파이프라인 전에는 붙을 호스트가 없다) |
| `dts/mediatek/` | `mt6897.dtsi`, `mt6897-lenovo-tb375fc.dts`, `mt6897-lenovo-tb373fu.dts`. 값의 출처와 할 일은 `dts/README.md` |

## 빌드

빌드 VM(Lima `buntos-dev`, Debian 13 arm64) 안에서 한다. 처음 한 번은 빌드 도구를 설치한다.

```bash
limactl shell buntos-dev -- sudo apt-get install -y flex bison bc libssl-dev libelf-dev dwarves cpio kmod lz4 device-tree-compiler debhelper libdw-dev
```

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'kernel/tb375fc/build.sh all'
```

| 단계 | 하는 일 |
|---|---|
| `prepare` | 원본을 받아 작업 트리(`~/buntos-build/tb375fc/linux-7.2.12-tb375fc`)를 만들고 패치·장치 트리를 얹는다. 작업 트리는 git으로 원본을 기억해 두었다가 매번 되돌린 뒤 얹는다 |
| `config` | arm64 defconfig에 조각을 합치고, 조각의 값이 그대로 들어가지 않은 기호를 알린다 |
| `dtbs` | 장치 트리만 빌드한다 |
| `kernel` | Image, 장치 트리, 모듈을 빌드하고 모듈을 `~/buntos-build/tb375fc/modules`에 설치한다 |
| `image-config` | 이미지용 작업 트리(`~/buntos-build/tb375fc/linux-7.2.12-image`)를 `prepare`처럼 만들고, Debian forky 설정에 조각을 합친다(`out-image/.config`). 디버그 섹션은 zstd로 줄인다. 조각의 값이 하나라도 빠지면 멈춘다 |
| `image` | `image-config` 뒤 컴파일하고 커널 .deb를 만든다. `deb/`에 linux-image·linux-headers, `deb/extra/`에 -dbg·linux-libc-dev. 끝나면 `out-image/`를 지운다 |

- 개발 빌드는 arm64 defconfig에 조각을 얹는다. 빠르고 플랫폼 드라이버가 넉넉하다.
- 이미지용 커널은 Debian forky 커널 설정에 같은 조각을 얹어 `build.sh image`로 빌드한다. 자기 작업 트리와 출력(`linux-7.2.12-image`, `out-image/`)을 써서 공용 작업 트리의 잠금을 잡지 않는다. 빌드 VM에서 컴파일 약 54분, 꾸러미 약 9분이고, 빌드 디스크는 작업 트리 2.4GB와 출력 17GB가 정점이다(맥 여유가 약 28GB일 때 시작한다). 꾸러미는 `linux-image-7.2.12-tb375fc`(판 `7.2.12-<n>+g<커밋>`)이고, 커널은 압축하지 않은 Image를 `/boot/vmlinuz-<판>`으로 싣는다. 디버그 정보는 Debian 그대로이고 디버그 섹션만 zstd로 줄인다(`DEBUG_INFO_COMPRESS=none`이면 줄이지 않는다). `debian-config.sh`는 설정만 만들어 본다(`out-debian/.config`). 기본은 패치 없는 원본 트리라 대기열의 기호는 따로 알리고, `KERNEL_TREE`·`KERNEL_OUT`·`STRICT=1`로 바꿀 수 있다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'kernel/tb375fc/debian-config.sh'
```

산출물(저장소 밖, `~/buntos-build/tb375fc/out`):
- `arch/arm64/boot/Image`
- `arch/arm64/boot/dts/mediatek/mt6897-lenovo-tb375fc.dtb`

부트 이미지(boot·vendor_boot·init_boot·dtbo)는 `bsp/lenovo-tb375fc/boot/`가 이것으로 만든다.

## 단계와 커널

| 단계 | 커널 쪽 | 상태 |
|---|---|---|
| 2a 첫 부팅 | UART(8250_mtk), simpledrm 화면 콘솔, pstore, 워치독, LZ4 initramfs | 설정 조각과 장치 트리에 있음. framebuffer·ramoops 노드는 기기에서 주소를 확인한 뒤 켠다 |
| 2b 저장장치·USB | UFS(ufs-mediatek), mtu3·xHCI·xsphy, USB 가젯, 핀·EINT, SPMI, MT6363·MT6319, cpufreq-hw | 핀·클럭·MFD·MT6363 키·UFS 드라이버와 SPMI 고침은 패치 대기열에 있고, pio·클럭 컨트롤러·SPMI(M 버스)·MT6363·MT6368·MT6319·UFS·USB 노드는 장치 트리에 있다(꺼 둠). USB를 켤 때는 SPMI와 MT6368도 켜야 한다(`vusb33`). UFS·USB에는 전원 도메인이 없다 |
| 2c 입력 | gpio-keys(홀), SPI, I2C, serdev | NT36532 터치·펜(0030·0031), 포고 키보드(0032~0036)는 패치 대기열에, SPI 여덟·터치·홀 센서·패널 VDDIO·키보드 노드는 장치 트리에 있다(꺼 둠) |
| USB 호스트 | I2C5·I2C10, MT6375 OTG, 차지 펌프 입력 FET, mtu3 otg 모드 | 드라이버는 패치 대기열(0042~0049)에, 노드는 장치 트리에 있다(꺼 둠). UFS 루트와 USB 가젯이 기기에서 된 뒤에 켠다. TCPC 전까지 역할과 VBUS는 사용자 공간에서 손으로 켠다 |
| 2d 무선 | 그 전에는 USB 동글(mt7921u)·USB 이더넷 | soc7_0(LK가 실은 펌웨어를 쓴다), conninfra, BT BEIF 전송은 패치 대기열([Wi-Fi 설계](../../docs/tb375fc/wifi-soc7_0.md)). 전원 도메인은 패치 0050~0054와 `scpsys` 노드(꺼 둠, conn 하나)가 있다 |
| 2e GPU·전원 | panthor, GPUEB 메일박스·MFG 전원 도메인, LVTS, MM8013 | MT6897 GPUEB 변형은 패치 대기열 |
