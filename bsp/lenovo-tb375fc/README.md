# TB375FC BSP

TB375FC(와 같은 하드웨어인 TB373FU)의 LK가 읽는 부트 이미지와 첫 부팅용 initramfs를 만든다. 꼴은 [설계안](../../docs/tb375fc/DESIGN.md) 2.2절을 따르고, 원래 펌웨어(글로벌판 ZUI 17.0.04.266)에서 읽은 값은 [하드웨어 실측](../../docs/hw/lenovo-tb375fc.md) 2절에 있다.

| 곳 | 내용 |
|---|---|
| `boot/mkimages.sh` | boot·init_boot·vendor_boot·dtbo를 만든다. 헤더 v4, 커널은 LZ4 legacy, vendor_boot의 DTB 영역은 DT 표(`dt_table.py`), dtbo는 빈 오버레이 하나 |
| `boot/test.sh` | 넷을 만들어 다시 풀고 넣은 것과 바이트를 견준다. 원래 이미지가 있으면 머리 필드도 견준다(`check_images.py`) |
| `initramfs/` | busybox initramfs. 화면·UART에 셸을 띄우고, 직전 부팅의 pstore 기록을 떠 두고, USB 가젯(ACM + NCM, 기기 172.16.42.1)을 켜고, nvdata·nvcfg·persist를 읽기 전용으로 붙인다 |
| `firmware/` | 기기에서 받은 원천(파티션 백업, vendor 파일)에서 펌웨어와 보정값을 고른다(`manifest.tsv`). 뽑은 파일은 저장소에 넣지 않는다 |

## 만들기

빌드 VM(`buntos-dev`)에서 한다. 커널 빌드 도구(`kernel/tb375fc/README.md`)에 더해 다음이 필요하다.

```bash
limactl shell buntos-dev -- sudo apt-get install -y busybox-static mkbootimg
```

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

## 기기에서 띄우기 전에

- 언락과 굽기는 buntOS 설계안 4.2.3절의 go/no-go 넷을 통과한 뒤에 한다. 파티션 이름과 슬롯은 기기에서 `fastboot getvar all`로 확인한다.
- **`fastboot boot`와 헤더 v4.** `fastboot boot`는 boot.img 하나만 RAM으로 보낸다. 헤더 v4에서 장치 트리는 vendor_boot에, initramfs는 init_boot에 있으므로, LK는 이 둘을 굽혀 있는 파티션(원래 것)에서 읽을 가능성이 높다(AOSP 부트로더의 동작이다. MT6897 LK에서는 확인하지 않았다).
  - 그러면 우리 커널이 원래 장치 트리로 뜬다. `--boot-ramdisk`로 만든 boot.img는 initramfs를 boot.img에도 넣어, LK가 그 램디스크를 쓰는지 보게 한다.
  - init은 vendor_boot 표지(`/etc/buntos/vendor_boot`)로 LK가 우리 vendor_boot를 읽었는지 알린다.
  - 우리 장치 트리로 띄우는 확실한 길은 비운 슬롯에 넷을 굽는 것이다(설계안 2.4절).
- 원래 이미지와 달리 AVB 꼬리, 파티션 크기 채움, dtbo 뒤의 MediaTek 서명 블록(cert1·cert2)이 없다. LK가 언락 상태에서 이것을 요구하는지는 기기에서 본다.
- 기본 커널 명령줄은 `console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused`다. initramfs 옵션(`buntos.wait`, `buntos.usbnet`, `buntos.telnet`, `buntos.debug`)은 `initramfs/init` 머리말에 있다.
