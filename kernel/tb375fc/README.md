# TB375FC 커널

TB375FC(Lenovo Xiaoxin Pad Pro 12.7 2025, MediaTek MT6897)용 mainline 커널 커스텀이다. 설계는 [docs/tb375fc/DESIGN.md](../../docs/tb375fc/DESIGN.md)의 1절(D1, D2), 3절, 4절, 5절을 따른다.

- **기반:** Linux **7.2.12** 원본(kernel.org)이다. buntOS 이미지의 Debian forky 커널과 같은 판이다. 벤더 GKI 커널은 쓰지 않는다.
- **커스텀:** 원본 위에 세 가지를 얹는다. 모두 업스트림에 낼 수 있는 꼴로 짠다.
  - 장치 트리(`dts/`)
  - 패치 대기열(`patches/`)
  - 보드 설정 조각(`config/`)

| 곳 | 내용 |
|---|---|
| `build.sh` | 원본 받기(sha256 확인), 작업 트리에 패치·장치 트리 얹기, 설정 합치기, 빌드 |
| `config/tb375fc.config` | 보드 설정 조각. 단계(2a~2e, 3)별로 묶었다. 7.2.12에 있는 기호만 쓴다 |
| `patches/series` | 패치 대기열과 다음 차례. 지금은 0001(장치 트리를 빌드 목록에 더하기)뿐이다 |
| `dts/mediatek/` | `mt6897.dtsi`, `mt6897-lenovo-tb375fc.dts`, `mt6897-lenovo-tb373fu.dts`. 값의 출처와 할 일은 `dts/README.md` |

## 빌드

빌드 VM(Lima `buntos-dev`, Debian 13 arm64) 안에서 한다. 처음 한 번은 빌드 도구를 설치한다.

```bash
limactl shell buntos-dev -- sudo apt-get install -y flex bison bc libssl-dev libelf-dev dwarves cpio kmod lz4 device-tree-compiler
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

- 개발 빌드는 arm64 defconfig에 조각을 얹는다. 빠르고 플랫폼 드라이버가 넉넉하다.
- 이미지에 넣을 때는 Debian forky 커널 설정에 같은 조각을 얹는다(나중에 `bsp/lenovo-tb375fc`에서).

산출물(저장소 밖, `~/buntos-build/tb375fc/out`):
- `arch/arm64/boot/Image`
- `arch/arm64/boot/dts/mediatek/mt6897-lenovo-tb375fc.dtb`

부트 이미지(boot·vendor_boot·init_boot·dtbo)는 `bsp/lenovo-tb375fc/boot/`가 이것으로 만든다.

## 단계와 커널

| 단계 | 커널 쪽 | 상태 |
|---|---|---|
| 2a 첫 부팅 | UART(8250_mtk), simpledrm 화면 콘솔, pstore, 워치독, LZ4 initramfs | 설정 조각과 장치 트리에 있음. framebuffer·ramoops 노드는 기기에서 주소를 확인한 뒤 켠다 |
| 2b 저장장치·USB | UFS(ufs-mediatek), mtu3·xHCI·xsphy, USB 가젯, SPMI, MT6363·MT6319, cpufreq-hw | 드라이버는 업스트림에 있음. 핀·클럭 데이터, MFD, SPMI 고침이 패치 대기열에서 필요. UFS·USB에는 전원 도메인이 없다 |
| 2c 입력 | gpio-keys(홀), SPI, I2C, serdev | NT36532 SPI 터치와 포고 키보드는 패치 대기열 |
| 2d 무선 | 그 전에는 USB 동글(mt7921u)·USB 이더넷 | soc7_0(LK가 실은 펌웨어를 쓴다), conninfra, BT BEIF 전송은 패치 대기열([Wi-Fi 설계](../../docs/tb375fc/wifi-soc7_0.md)) |
| 2e GPU·전원 | panthor, GPUEB 메일박스·MFG 전원 도메인, LVTS, MM8013 | MT6897 GPUEB 변형은 패치 대기열 |
