# TB375FC BSP

TB375FC(와 같은 하드웨어인 TB373FU)의 LK가 읽는 부트 이미지와 첫 부팅용 initramfs를 만든다. 꼴은 [설계안](../../docs/tb375fc/DESIGN.md) 2.2절을 따르고, 원래 펌웨어(글로벌판 ZUI 17.0.04.266)에서 읽은 값은 [하드웨어 실측](../../docs/hw/lenovo-tb375fc.md) 2절에 있다.

| 곳 | 내용 |
|---|---|
| `boot/mkimages.sh` | boot·init_boot·vendor_boot·dtbo를 만든다. 헤더 v4, 커널은 LZ4 legacy, vendor_boot의 DTB 영역은 DT 표(`dt_table.py`), dtbo는 빈 오버레이 하나 |
| `boot/test.sh` | 넷을 만들어 다시 풀고 넣은 것과 바이트를 견준다. 원래 이미지가 있으면 머리 필드도 견준다(`check_images.py`) |
| `initramfs/` | busybox initramfs. 화면·UART에 셸을 띄우고, 직전 부팅의 pstore 기록을 떠 둔다. 명령줄에 `root=`가 있으면 그 파티션으로 넘어간다(`switch_root`). 못 넘어가면 까닭을 남기고 USB 가젯(ACM + NCM, 기기 172.16.42.1) 셸로 남아 nvdata·nvcfg·persist를 읽기 전용으로 붙인다 |
| `initramfs/test.sh` | 루트 전환을 QEMU virt에서 시험한다. 쪽 4096바이트 GPT 디스크, 넘어가지 못하는 경우 열아홉 가지, `--root`를 주면 systemd·가젯·SSH까지 본다. 시험 이미지 설정은 `initramfs/test-image/` |
| `mkosi.extra/` | 진짜 루트에 넣는 것: 개발용 USB 길(가젯 스크립트, 서비스 셋), 프리셋, apt 보호(아래 절), 파일 시스템 늘리기 설정, UFS를 데스크톱에서 가리는 udev 규칙(`usr/lib/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules`), 부팅 때 드라이버를 올리는 서비스(`buntos-display`, `buntos-gpu`, `buntos-wifi`, `buntos-charge`) |
| `udev/test.sh` | UFS 가리기 규칙을 시험한다. 인자 없이 돌면 몇 초 만에 `udevadm verify`와 규칙 검사(UDISKS_* 말고는 정하지 않는지, 규칙이 기대는 장치 트리 노드·드라이버 이름이 소스에 있는지)를 본다. `--root`를 주면 QEMU virt에서 UFS(LU 둘, 4096바이트 쪽 GPT에 안드로이드를 흉내 낸 파티션과 buntOS 루트)와 USB 메모리를 함께 달아 initramfs로 루트에 넘어간 뒤, 진짜 udev가 UFS에만 값을 붙이는지, `/dev/disk/by-partlabel`·nvdata `ro,noload` 붙이기·슬롯 굽기가 그대로인지, 이미지에 udisks2가 있으면 gvfs가 UFS 볼륨을 내놓지 않는지까지 본다 |
| `mkosi.postinst.chroot` | systemd-repart와 gpt-auto 생성기를 가리고, 파일 시스템 늘리기를 켠다. 이 기기에서는 GPT를 고치면 안 된다 |
| `firmware/` | 기기에서 받은 원천(파티션 백업, vendor 파일)에서 펌웨어와 보정값을 고른다(`manifest.tsv`). 뽑은 파일은 저장소에 넣지 않는다 |
| `mesa/build.sh` | Mesa 26.3을 이미지와 같은 Debian 스냅숏의 컨테이너에서 빌드해 꾸러미로 만든다 |
| `upower/` | 직충전 중에도 남은 시간이 나오도록 고친 UPower 데몬을 같은 방식으로 꾸러미로 만든다(패치와 `build.sh`) |
| `mkosi.conf` | mkosi 보드 프로필(`tools/image/build.sh lenovo-tb375fc`). Debian 커널 대신 `kernel/tb375fc/build.sh image`가 만든 커널 .deb를 넣고, 루트 파티션 파일과 UKI에서 꺼낸 커널·initrd를 따로 낸다 |


## 부팅 때 올리는 드라이버

명령줄의 `modprobe.blacklist`가 자동 적재를 막고 서비스가 차례를 정한다. 모두 프리셋으로 켜져 있다.

| 서비스 | 하는 일 | 차례 |
|---|---|---|
| `buntos-display.service` | PHY, mtk-mmsys, 패널, mediatek-drm을 올리고 DSI 커넥터를 기다린다. 화면 노드가 꺼진 커널이면 simple-framebuffer 그대로 둔다 | 로그인 화면 전 |
| `buntos-gpu.service` | panthor를 올리고 렌더 노드를 기다린다. 안 뜨면 CPU로 그린다 | 로그인 화면 전 |
| `buntos-wifi.service` | mt7921soc를 올린다. NetworkManager가 wlan0을 받는다 | 로그인 화면 뒤 |
| `buntos-bluetooth.service` | `btmtkbeif`를 올리고 `bluetooth-up`이 어댑터에 주소를 넣는다. 주소는 처음 켤 때 만들어 `/var/lib/buntos/bluetooth-address`에 둔다. 공장 주소를 쓰려면 이 파일을 바꾼다 | Wi-Fi 뒤 |
| `buntos-sound.service` | AFE, MT6368 코덱, 앰프, 카드를 올리고 `persist`의 앰프 보정값을 읽기 전용으로 읽어 넣는다. 못 읽으면 드라이버가 앰프를 -30 dB 아래로 묶는다 | 로그인 화면 뒤 |
| `buntos-charge.service` | 온도 감시(`charge-guard`: 배터리 40도, 커넥터 55도)를 먼저 걸고 충전 드라이버를 올린다. 끄려면 `systemctl stop`(이번 부팅), `systemctl disable`(다음 부팅부터) | 로그인 화면 뒤 |

전원 키: `logind.conf.d/50-tb375fc.conf`가 누르면 잠들게 하고, 10초 잡기는 커널이 한다(패치 0186). 컴퓨터에 꽂힌 USB 가젯은 컨트롤러가 잠들지 못하게 해서 `system-sleep/buntos-usb-gadget`가 잠든 동안 뗀다. 자세한 것은 [체크리스트 9.7절](../../docs/tb375fc/bringup-checklist.md)이다.

잠든 동안 충전: 충전기가 꽂혀 있으면 잠자기가 `libexec/buntos/charge-sleep`으로 간다(`systemd-suspend.service.d/50-buntos-charge-sleep.conf`). 순정처럼 RTC 알람으로 10초마다 화면 없이 깨어, 충전 드라이버가 온도와 전압을 다시 보고 깨어 있을 때와 같이 충전하게 한 뒤 다시 잠든다(패치 0205~0208). 차지 펌프가 도는 동안은 잠들지 않고 화면만 끈 채 깨어 있다. 키, 덮개, 충전기 뽑기에 끝나고, 기록은 `/var/log/buntos-charge/sleep.log`에 남는다. `charge-guard`의 30초 타이머는 잠든 시간을 세지 않으므로, `buntos-charge.service`가 도는 동안은 `charge-sleep`이 세 번 깰 때마다(30초) `charge-guard check`로 한 번 보게 한다. 충전기가 없으면 예전과 같은 잠자기다.

Wi-Fi, 소리, 충전은 `multi-user.target` 앞이라, 적재에서 멈추면 슬롯의 재시도가 줄어 안드로이드로 돌아갈 길이 남는다(`buntos-slot-keep.service`).

GPU의 클럭은 평소 542 MHz까지만 쓴다(`mkosi.extra/usr/lib/modprobe.d/90-buntos-tb375fc-gpu.conf`, 그 위에서는 전원부 소리가 난다). 스크롤처럼 다음 프레임을 곧바로 그리는 일은 프레임이 떨어져도 드라이버가 이 상한을 풀지 않으므로, 컴포지터의 프레임 바닥이 GPU의 최소 클럭을 올린다: 사람이 화면을 움직이는 동안 프레임이 초당 60 아래로 떨어지면 올리고, 지켜지거나 움직임이 멈추면 푼다(buntOS 설계안 8.2절). `mkosi.extra/usr/lib/udev/rules.d/90-buntos-tb375fc-gpu.rules`가 그 파일(devfreq `min_freq`)을 render 그룹에 연다. 올릴 수 있는 클럭을 묶으려면 `/etc/buntos/comp.toml`의 `[gpu] min-fps-max-mhz`를 쓴다.

GPU에는 Mesa 26.3이 있어야 한다(G615는 26.3부터 돈다. 이미지의 Debian 스냅숏은 26.1이다). `mesa/build.sh`가 꾸러미 `buntos-mesa`로 만들어 `/opt/mesa`에 넣고 `/etc/ld.so.conf.d/00-opt-mesa.conf`로 Debian의 Mesa보다 먼저 찾게 한다. GPU 펌웨어(`arm/mali/arch11.8/mali_csffw.bin`)는 Debian의 `firmware-misc-nonfree`에 있다.

UPower 1.91.5는 PPS 충전기를 쓰는 동안 USB 포트의 `online` 값 2를 받지 못해 충전 속도를 0으로 둔다. 그래서 직충전 중에는 남은 시간이 나오지 않는다. `upower/build.sh`가 고친 데몬을 꾸러미 `buntos-upower`로 만들어 `/opt/upower`에 넣고, systemd drop-in이 그 데몬을 띄운다. Debian의 `upower`는 그대로 둔다. 꾸러미를 지우면 Debian의 데몬으로 돌아가고, UPower에 이 고침이 들어오면 폴더와 `mkosi.conf`의 항목을 지운다.

레시피에 넣지 않은 것: 터치 펌웨어, Wi-Fi 보정 파일, 스피커 앰프의 튜닝 파일(`/usr/lib/firmware/lenovo/tb375fc/`). 제조사와 기기의 파일이라 저장소에 넣지 않는다. 이미지를 구운 뒤 따로 넣는다.

## 만들기

빌드 VM(`buntos-dev`)에서 한다. 커널 빌드 도구(`kernel/tb375fc/README.md`)에 더해 다음이 필요하다.

```bash
limactl shell buntos-dev -- sudo apt-get install -y busybox-static mkbootimg
```

명령은 맥에서 저장소 맨 위 폴더에서 실행한다(VM은 저장소를 같은 경로로 마운트한다).

1. 커널: `kernel/tb375fc/build.sh all`
2. initramfs: 커널 모듈을 함께 넣는다. 출력은 `~/buntos-build/tb375fc/images/initramfs.cpio.lz4`다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'bsp/lenovo-tb375fc/initramfs/mkinitramfs.sh --modules ~/buntos-build/tb375fc/modules'
```

3. 부트 이미지: 출력은 `~/buntos-build/tb375fc/images/`의 이미지 넷과 `images.txt`(입력, 명령줄, 해시)다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'K=~/buntos-build/tb375fc/out/arch/arm64/boot && bsp/lenovo-tb375fc/boot/mkimages.sh --kernel $K/Image --dtb $K/dts/mediatek/mt6897-lenovo-tb375fc.dtb --initramfs ~/buntos-build/tb375fc/images/initramfs.cpio.lz4'
```

4. 시험: 입력을 주지 않으면 가짜 커널·트리·cpio로 돈다. 원래 이미지는 `.handoff/tb375fc/stock/`에서 찾는다(레노버 파일이라 저장소에 넣지 않는다).

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'bsp/lenovo-tb375fc/boot/test.sh'
```

5. 맛보기: 기기 없이 같은 커널과 initramfs를 빌드 VM의 QEMU virt로 띄워 init이 끝까지 도는지 본다. 장치 트리는 QEMU 것이라 vendor_boot 표지, UDC, nvdata는 없다고 나온다. 셸에서 `poweroff -f`로 끝낸다.

```bash
limactl shell buntos-dev -- bash -lc 'B=~/buntos-build/tb375fc; qemu-system-aarch64 -M virt -cpu host -accel kvm -smp 2 -m 1024 -nographic -no-reboot -kernel $B/out/arch/arm64/boot/Image -initrd $B/images/initramfs.cpio.lz4 -append "console=ttyAMA0 buntos.wait=3"'
```

LK처럼 vendor_boot의 램디스크 조각을 initramfs 앞에 이어 붙여 `-initrd`로 주면 init이 표지를 알아본다(2026-11-01 확인).

6. 루트 전환 시험: QEMU virt(KVM)에서 명령줄을 바꿔 가며 띄운다. 빌드 디스크를 약 1GB 쓰고, 끝나면 기록(`log/`)만 남긴다. `--root`까지 주면 약 11분 걸린다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'bsp/lenovo-tb375fc/initramfs/test.sh'
```

systemd까지 보려면 작은 시험 이미지를 만들어 `--root`로 준다(`initramfs/test-image/mkosi.conf`, 루트 파일 약 1.3GB).

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'O=~/buntos-build/tb375fc/initramfs-verify/mkosi-out && mkosi -C bsp/lenovo-tb375fc/initramfs/test-image --include="$PWD/bsp/lenovo-tb375fc" --output-directory=$O -f build && bsp/lenovo-tb375fc/initramfs/test.sh --root $O/buntos.root-arm64.raw'
```

7. UFS 가리기 규칙 시험: 규칙 검사는 몇 초면 된다. `--root`를 주면 QEMU에서 UFS와 USB 메모리를 달아 본다(빌드 디스크를 1GB 안팎 쓰고, 끝나면 기록만 남긴다. 호스트가 한가하면 3~4분이다). 6번의 작은 시험 이미지나 전체 이미지의 루트 파티션 파일을 줄 수 있다. 보드 프로필(`--include`)로 만든 것이어야 규칙이 들어 있다. 이미지에 udisks2·gvfs·dbus가 있으면 gvfs와 udisksctl 시험도 한다.

   ```bash
   limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'bsp/lenovo-tb375fc/udev/test.sh'
   limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'bsp/lenovo-tb375fc/udev/test.sh --root <루트 파티션 파일>'
   ```

## 이미지

buntOS 이미지는 공통 레시피(`image/`)에 이 폴더의 `mkosi.conf`를 얹어 만든다.

1. 커널 꾸러미: `kernel/tb375fc/build.sh image`(출력 `~/buntos-build/tb375fc/deb/`)
2. Mesa 꾸러미: `bsp/lenovo-tb375fc/mesa/build.sh`(출력 `~/buntos-build/tb375fc/mesa-deb/`). 처음 약 20분, 디스크 약 5GB다. 판이 그대로면 다시 돌리지 않아도 된다
3. UPower 꾸러미: `bsp/lenovo-tb375fc/upower/build.sh`(출력 `~/buntos-build/tb375fc/upower-deb/`). 처음 몇 분 걸리고, 판이 그대로면 다시 돌리지 않아도 된다
4. 이미지: `tools/image/build.sh lenovo-tb375fc`. 맥 여유가 30GB 이상일 때 한다(자체 컴포넌트를 cargo로 빌드하고 mkosi가 최대 약 14GB를 쓴다)

- 공통 레시피의 `Bootable=yes`, `Bootloader=systemd-boot`는 보드 프로필에서 바꿀 수 없다(mkosi가 보드 프로필을 먼저 읽는다). 그래서 이미지에는 ESP, systemd-boot, UKI가 그대로 생긴다. U-Boot로 EFI 부팅을 하게 되면(설계안 D3) 그대로 쓴다.
- 커널 꾸러미는 커널을 `/boot/vmlinuz-<판>`에 두고, mkosi가 그것을 `/usr/lib/modules/<판>/vmlinuz`로 복사해 UKI를 만든다.
- LK로 띄울 때는 루트 파티션(`buntos.root-arm64.raw`)만 기기에 굽는다. 부트 이미지는 같은 커널 꾸러미의 Image·장치 트리·모듈로 `boot/mkimages.sh`가 만든다(꾸러미를 `dpkg-deb -x`로 풀어 `--kernel`, `--dtb`, `mkinitramfs.sh --modules`에 준다).
- 이 기기의 UFS에는 안드로이드 파티션이 함께 있다. 공통 레시피는 처음 켤 때 `systemd-repart`로 GPT를 고쳐 루트를 늘리는데, 보드 프로필은 이것을 가린다(`mkosi.postinst.chroot`). GPT 형식 GUID로 파티션을 붙이는 `systemd-gpt-auto-generator`도 가린다. EFI가 아니어도 도는 생성기다. 작게 구운 btrfs는 `systemd-growfs-root.service`로 파일 시스템만 파티션 끝까지 늘린다. initramfs도 넘어가기 전에 `/run`에서 같은 것을 가리므로, 다른 보드의 이미지를 잘못 구워도 GPT를 고치지 않는다.
- 공통 레시피와 달리 `systemctl disable`은 듣지 않는다. mkosi가 설치 뒤 스크립트 다음에 `preset-all`을 돌려 되살린다. 그래서 켜고 끄는 것은 프리셋(`mkosi.extra/usr/lib/systemd/system-preset/`)으로 정한다.
- UFS의 안드로이드 파티션과 nvdata는 데스크톱에서 가린다. 전체 이미지에는 udisks2가 들어와(nautilus → gvfs) 가리지 않으면 파일 관리자에 볼륨으로 보인다. `mkosi.extra/usr/lib/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules`가 UFS 호스트 아래의 모든 블록 장치(buntOS 루트 포함)에 `UDISKS_IGNORE=1`을 준다. 이름 목록이 아니라 장치로 고르므로 파티션 이름을 몰라도 빠지지 않고, USB 저장장치는 그대로 보인다. 숨김일 뿐이다: udisksctl로 직접 붙이는 일은 막지 않는다(polkit이 따로 필요). `/dev/disk/by-partlabel` 링크, nvdata를 `ro,noload`로 붙이는 길, 슬롯 굽기에는 영향이 없다.

## 루트로 넘기기

`root=`는 `mkimages.sh` 기본값에 넣지 않는다. 파티션 이름은 기기에서 정하므로 `--cmdline`으로 준다. 기본값에 아래를 더한다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'x=$(mktemp -d) && dpkg-deb -x ~/buntos-build/tb375fc/deb/linux-image-7.2.12-tb375fc_*_arm64.deb $x && bsp/lenovo-tb375fc/initramfs/mkinitramfs.sh --modules $x && bsp/lenovo-tb375fc/boot/mkimages.sh --kernel $x/boot/vmlinuz-7.2.12-tb375fc --dtb $x/usr/lib/linux-image-7.2.12-tb375fc/mediatek/mt6897-lenovo-tb375fc.dtb --initramfs ~/buntos-build/tb375fc/images/initramfs.cpio.lz4 --cmdline "console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused regulator_ignore_unused root=PARTLABEL=<이름> rootfstype=btrfs rw systemd.firstboot=no buntos.usbdebug=1 systemd.set_credential_binary=ssh.authorized_keys.root:$(base64 -w0 <공개 키 파일>)"'
```

| 값 | 뜻 |
|---|---|
| `root=PARTLABEL=<이름>` | 넘어갈 파티션. `PARTUUID=`, `UUID=`, `/dev/…`도 받는다. 형식 GUID는 root-arm64다 |
| `rootfstype=btrfs`, `rw` | 없으면 mount가 형식을 찾고 읽기 전용으로 붙인다. LK가 `ro`를 덧붙여도 `rw`가 이긴다 |
| `systemd.firstboot=no` | 공통 레시피가 UKI에 넣는 값과 같다. 없으면 처음 켤 때 `systemd-firstboot`가 화면에서 입력을 기다린다 |
| `buntos.usbdebug=1` | 진짜 루트의 개발용 USB 길을 켠다. 출시 부트 이미지에는 넣지 않는다 |
| `systemd.set_credential_binary=ssh.authorized_keys.root:…` | root의 SSH 공개 키. 한 번 들어가면 루트에 남는다. ed25519를 쓴다. RSA 4096은 명령줄 한도(boot v4 1536바이트)에 빠듯하다 |

LK가 자기 `root=`를 덧붙여도(순정 트리의 `/chosen`은 `root=/dev/ram`이다) 우리 값으로 넘어간다. `PARTLABEL=`, `PARTUUID=`, `UUID=`가 `/dev/` 경로보다 먼저이고, `/dev/ram*`, `/dev/dm-*`, `/dev/root`, `/dev/nfs`는 버린다. `init=/init`도 버린다.

못 넘어가면 패닉 없이 셸이 뜬다. 까닭은 화면, dmesg, pstore, `/run/buntos/boot.log`에 남고, 셸을 열면 먼저 보인다. 찾지 못했으면 `/run/buntos/block.txt`에 장치와 GPT 이름 목록이 있다. 넘어간 뒤에도 `/run/buntos`의 기록은 남는다.

호스트에서 들어가기(`buntos.usbdebug=1`):

```bash
ssh root@172.16.42.1
```

공개 키가 아직 없거나 네트워크가 안 되면 직렬 포트의 암호 없는 root 셸을 쓴다. 호스트의 장치 이름은 macOS `/dev/cu.usbmodem*`, Linux `/dev/ttyACM0`이다.

```bash
screen /dev/ttyACM0 115200
```

셋 다 다른 서비스를 기다리지 않아, sysinit이 멈추거나 emergency.target으로 넘어가도 들어갈 수 있다. 호스트는 DHCP로 172.16.42.2를 받는다. 가젯은 진짜 루트에서 새로 띄운다. initramfs의 가젯은 실패 길에서만 돈다. 그래서 넘어가는 동안 USB가 잠깐 없다.

## 기기에서 띄우기 전에

- 언락과 굽기는 buntOS 설계안 4.2.3절의 go/no-go 넷을 통과한 뒤에 한다. 백업과 복구는 SP Flash Tool과 그 판의 순정 펌웨어 묶음(인증 DA·auth)으로 하고, 언락은 판에 묶인 커뮤니티의 길로 한다([TB375FC 설계안](../../docs/tb375fc/DESIGN.md) 2.1절). 파티션 이름과 슬롯은 기기에서 `fastboot getvar all`로 확인한다.
- **`fastboot boot`와 헤더 v4.** 이 기기의 LK에는 `fastboot boot`가 없다(기기가 `unknown command`로 거부, 명령 표에도 없다. 2026-11-03). 이미지는 비운 슬롯 `_b`에 굽는다. 아래는 다른 LK를 위한 설명이다. `fastboot boot`는 boot.img 하나만 RAM으로 보낸다. 헤더 v4에서 장치 트리는 vendor_boot에, initramfs는 init_boot에 있으므로, LK는 이 둘을 굽혀 있는 파티션(원래 것)에서 읽을 가능성이 높다. LK가 세 이미지의 머리를 모두 읽는 것은 LK 문자열로 확인했고, RAM으로 받은 boot.img와 어떻게 잇는지는 기기에서 본다.
  - 그러면 우리 커널이 원래 장치 트리로 뜬다. `--boot-ramdisk`로 만든 boot.img는 initramfs를 boot.img에도 넣어, LK가 그 램디스크를 쓰는지 보게 한다.
  - init은 vendor_boot 표지(`/etc/buntos/vendor_boot`)로 LK가 우리 vendor_boot를 읽었는지 알린다.
  - 우리 장치 트리로 띄우는 확실한 길은 비운 슬롯에 넷을 굽는 것이다(설계안 2.4절).
- 원래 이미지와 달리 AVB 꼬리, 파티션 크기 채움, dtbo 뒤의 MediaTek 서명 블록(cert1·cert2)이 없다. LK가 언락 상태에서 이것을 요구하는지는 기기에서 본다.
- 기본 커널 명령줄은 `console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused regulator_ignore_unused`다. 뒤의 셋은 소비자가 아직 없는 클럭·전원 도메인·레귤레이터를 늦은 정리에서 끄지 않게 한다(2b에서 PMIC 레일이 등록되면 소비자 없는 레일이 꺼질 수 있다). 다만 `pd_ignore_unused`는 늦은 정리(`genpd_power_off_unused`)만 막고 sync_state의 끄기는 막지 못한다(7.2.12). LK가 켠 도메인이 꺼지지 않는 것은 패치 0054의 표가 모두 기본 꺼짐이고 장치 트리에 conn만 있기 때문이다. MT6895 xaga 커뮤니티 포트는 LK가 boot·vendor_boot 이미지의 명령줄을 버린다고 적었다. 처음 띄우면 `/proc/cmdline`으로 들어왔는지 보고, 안 들어오면 커널 `CMDLINE`(+`CMDLINE_EXTEND`)로 준다. initramfs 옵션(`root=`와 그 짝, `buntos.wait`, `buntos.usbnet`, `buntos.telnet`, `buntos.debug`)은 `initramfs/init` 머리말에 있다.

## apt가 우리 것을 망가뜨리지 않게

기기와 이미지의 apt는 live forky를 보고 `apt upgrade`를 그대로 써도 된다. 우리 바이너리와 모듈은 dpkg가 모르는 파일이라 apt에게 의존성이 없고, 그래서 위험은 덮어쓰기보다 **쓰는 줄 모르고 라이브러리를 지우는 것**이다. dpkg가 모르는 파일 2375개를 forky의 전체 파일 목록과 맞대 보니 경로가 겹치는 것은 설치되지 않은 꾸러미 하나(`sxmo-utils`의 `mimeapps.list`)뿐이었다(2026-11-05).

도구와 공통 설정은 buntOS 공통 레시피(`image/`, 설계안 5.5절)에 있고, `image/mkosi.postinst.chroot`가 `/usr/libexec/buntos/apt-guard install`을 돌린다. 이 기기가 더 지키는 것은 `/etc/buntos/apt-guard.conf.d/50-tb375fc.conf`에 있다(커널 모듈, GPU와 패널 펌웨어, Mesa, 소리). 커널과 부트로더 차단(`preferences.d/70-buntos-tb375fc`)도 여기에만 있다.

| 무엇 | 어떻게 | 막는 것 |
|---|---|---|
| 꾸러미 `buntos-runtime` | 바이너리의 `DT_NEEDED`를 꾸러미 이름으로 바꿔 의존성으로 잡고 고정한다 | `autoremove`, `full-upgrade`가 라이브러리를 지우는 것 |
| 고정(hold) | 커널, `buntos-mesa`, `firmware-misc-nonfree`(GPU 펌웨어) | 올리거나 지우는 것. `dpkg -i`로 일부러 넣는 것은 된다. 그러면 고정이 풀리니 `apt-guard install`을 다시 돌린다 |
| `preferences.d/70-buntos-tb375fc` | Debian 커널과 다른 부트로더, initramfs 도구의 후보를 없앤다 | 부팅되지 않는 커널, 디스크를 건드리려는 도구 |
| `dpkg-divert` | `os-release`, `mimeapps.list` | 같은 경로의 꾸러미 파일은 `<경로>.dpkg-dist`로 받는다 |
| `/usr/sbin/policy-rc.d` | 로그인 화면, NetworkManager, `wpa_supplicant`, D-Bus, logind, polkit, UPower의 재시작과 멈춤을 막는다 | 쓰는 중에 화면이나 Wi-Fi가 끊겨 apt까지 함께 죽는 것. 새 판은 다음 부팅부터 돈다 |
| `Dpkg::Options` | `--force-confold` | 고친 설정 파일을 묻는 창에서 멈추는 것 |

그래도 일이 나면: apt가 dpkg를 부르기 전(`DPkg::Pre-Invoke`)에 btrfs 읽기 전용 스냅숏과 해시를 남기고, 끝나면(`DPkg::Post-Invoke`) 해시, 라이브러리, 서비스 켜짐 상태, Mesa가 읽히는 자리를 비교해 알린다. 패키지 매니저도 libapt를 쓰므로 함께 걸린다(PackageKit `InstallFiles`로 봤다). 기록은 `/var/log/buntos-apt-guard.log`다. 스냅숏은 맨 위 서브볼륨의 `snapshots/`에 3개까지 남고, 루트 여유가 3GB보다 적거나 스냅숏을 못 만들면 dpkg 앞에서 멈춘다.

```bash
apt-guard check                 # 지금 상태
apt-guard refresh               # 바이너리를 바꾼 뒤 의존성을 다시 계산
apt-guard restore               # post가 알린 파일을 가장 새 스냅숏에서 되돌림
apt-guard list                  # 스냅숏 목록
apt-guard rollback <스냅숏>      # 다음 부팅부터 루트 전체를 그때로. /home도 돌아간다
```

`apt-guard`는 `/usr/libexec/buntos/`에 있다. `BUNTOS_APT_NO_SNAPSHOT=1`을 붙이면 스냅숏을 건너뛴다.

**기기에서 본 것(2026-11-05):** `apt-get upgrade`로 39개를 실제로 올렸다. 지켜 보는 파일 1718개의 해시가 그대로이고, `wpa_supplicant` 재시작이 막혀 Wi-Fi와 세션이 끊기지 않았고, 스냅숏이 올리기 전 판을 담고 있다. Debian 커널 설치와 `libseat1` 제거, 커널 제거는 거부되고 `autoremove`는 지우는 것이 없다. 일부러 우리 파일을 덮어쓰는 꾸러미를 넣으면 `post`가 알리고 `restore`가 되돌린다. 올린 뒤 재부팅하면 실패한 서비스 없이 로그인 화면까지 뜨고 새 `wpa_supplicant`가 돈다. **못 본 것:** `rollback` 뒤의 부팅(이름 바꾸기와 스냅숏 풀기까지만 봤다), mkosi 이미지 안에서 `apt-guard install`.
