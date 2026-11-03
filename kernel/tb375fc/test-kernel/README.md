# TB375FC 시험 커널

장치 트리와 initramfs를 커널 안에 넣어, LK가 넘기는 트리를 쓰지 않는 커널이다. 2026-11-03에 사용자 기기에서 BusyBox 셸과 화면 콘솔까지 갔다. 결과는 [실측 보고서](../../../docs/hw/lenovo-tb375fc.md) 7절의 "첫 리눅스 부팅"에 있다.

**왜 있나.** 이 기기의 LK에는 `fastboot boot`가 없고, 서명 없는 `dtbo`를 거부하며, 우리 트리에 펌웨어 적재가 찾는 벤더 노드가 없으면 `app_load_scp`에서 멈춘다. 그래서 `_b`에 순정 `vendor_boot`와 `dtbo`를 두어 LK를 통과하고, 커널은 자기 트리를 쓴다. 정식 길이 아니라 시험용이다.

**한계.** LK가 더하는 값(메모리 크기, 예약 영역)을 받지 못한다. [mt6897-lenovo-tb375fc-test.dts](mt6897-lenovo-tb375fc-test.dts)의 값은 8GiB 기기의 16.0.20.091 LK에서 읽은 것이다. 다른 기기나 다른 LK에서는 맞지 않을 수 있다. UART는 레지스터 접근에서 멈출 수 있어 꺼 두었다.

| 파일 | 내용 |
|---|---|
| `gen_dts.py` | 안드로이드의 `/sys/firmware/fdt`에서 예약 영역을 읽어 시험 dts를 만든다. 원본 트리에는 식별자가 있어 저장소에 넣지 않는다 |
| `mt6897-lenovo-tb375fc-test.dts` | 위에서 만든 트리. 보드 dts를 포함하고 명령줄, 메모리, 예약 영역을 더한다 |
| `builtin-dtb.py` | 작업 트리의 `setup_machine_fdt()`를 고쳐 커널에 넣은 트리를 쓰게 한다 |
| `build-test.sh` | 빌드 VM에서 시험 커널과 `boot.img`를 만든다 |
| `flash-b-test.sh` | 맥에서 슬롯 `_b`에 굽는다 |

## 쓰는 법

빌드 VM에서 보통 커널과 initramfs를 만든 뒤 시험 커널을 만든다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'kernel/tb375fc/build.sh all && bsp/lenovo-tb375fc/initramfs/mkinitramfs.sh --modules ~/buntos-build/tb375fc/modules && kernel/tb375fc/test-kernel/build-test.sh'
```

맥에서 fastboot로 들어가 굽는다. 슬롯은 스크립트가 바꾸지 않는다.

```bash
kernel/tb375fc/test-kernel/flash-b-test.sh <묶음의 image 디렉터리> .handoff/tb375fc-test-kernel/images/boot.img
```

```bash
fastboot --set-active=b && fastboot reboot
```

## 기록 읽기

명령줄의 `buntos.autoreboot=15`가 따뜻한 재부팅을 반복하게 한다. 재시도를 다 쓰면 LK가 `_a`로 돌아가고, 안드로이드에서 root로 `/sys/fs/pstore/console-ramoops-0`을 읽으면 마지막 부팅의 기록 전체가 나온다. 전원 + 볼륨 올림으로 리셋하면 pstore가 비어 있다. `expdb` 앞에 LK의 kedump가 남아 있으면 부팅마다 옛 기록을 되살리므로 지워야 한다(실측 보고서 7절).

셸에 머물게 하려면 시험 dts에서 `buntos.autoreboot`를 빼고 다시 만든다. 빠져나오는 키는 전원 + 볼륨 올림이고, 그 뒤 `fastboot --set-active=a`로 안드로이드로 돌아간다.
