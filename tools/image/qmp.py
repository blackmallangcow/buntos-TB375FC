# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""QEMU QMP로 VM에 입력을 보내고 화면을 저장한다 (ADR-009의 Phase 0 드라이버).

사용법: python3 qmp.py <포트 또는 소켓 경로> <동작>...
동작:
  overlay:PATH     디스크에 임시 qcow2 덮어쓰기(PATH)를 씌운다. 빌드한 이미지는 읽기만 한다
  cont             멈춘 VM을 이어서 실행한다 (QEMU를 -S로 띄웠을 때)
  click:X,Y        화면 좌표(X, Y)를 왼쪽 버튼으로 누른다 (가상 태블릿 필요)
  dclick:X,Y       두 번 누른다 (예: 제목 표시줄을 두 번 눌러 최대화)
  rclick:X,Y       오른쪽 버튼으로 누른다 (단축 메뉴)
  drag:X1,Y1,X2,Y2 왼쪽 버튼을 누른 채 (X1, Y1)에서 (X2, Y2)로 끈다
  move:X,Y         포인터만 옮긴다
  press, release   왼쪽 단추를 누른 채로 두거나 뗀다 (끌기 중간에 화면을 찍을 때 move와 함께 쓴다)
  keys:A+B         키를 함께 누른다 (QEMU qcode 이름, 예: alt+ret)
  down:A, up:A     키를 누른 채로 두거나 뗀다 (⌘를 누른 채 ⇥를 거듭 누르는 앱 전환 등)
  type:TEXT        영문 소문자·숫자를 한 글자씩 입력한다
  wheel:DIR,N      포인터 자리에서 휠을 N눈금 돌린다 (DIR: up, down, left, right)
  touch:X,Y        화면 좌표(X, Y)를 손가락으로 누른다 (가상 터치 화면 필요: vmctl.sh의 VMCTL_TOUCH=1)
  tdrag:X1,Y1,X2,Y2[,STEPS]
                   손가락 하나로 (X1, Y1)에서 (X2, Y2)로 끈다. STEPS번에 나눠 옮긴다 (기본 12)
  sleep:SECONDS    기다린다
  screen:PATH      화면을 PNG로 저장한다 (경로는 QEMU가 보는 경로)
  dark:PATH,X1,Y1,X2,Y2
                   화면을 PATH에 PPM으로 저장하고 그 사각형의 어두운 점 수를 찍는다 (밝은 판 위의 그림이 있는지 볼 때)
  plug:DRIVER,ID,BUS
                   장치를 PCIe 포트에 붙인다 (device_add. 입력 모드 시험)
  unplug:ID        장치를 뗀다 (device_del. 게스트가 몇 초 뒤에 떼어 낸다)
"""

import json
import socket
import sys
import time

# QMP 절대 좌표 범위 (가상 태블릿과 가상 터치 화면이 같다)
ABS_MAX = 0x7FFF
# dark: 어두운 점으로 보는 R+G+B의 상한 (밝은 판 위의 검은 그림. 안티에일리어싱한 가장자리는 세지 않는다)
DARK = 300
# 손가락 하나만 쓴다. QEMU의 GTK 화면처럼 손가락 번호(tracking id)도 슬롯 번호로 둔다
TOUCH_SLOT = 0
# mkosi가 이미지 디스크에 붙이는 블록 노드 이름
DISK_NODE = "mkosi"


def connect(address, timeout=60):
    """QEMU가 막 뜨는 중이면 QMP 소켓이 열릴 때까지 기다린다."""
    if address.isdigit():
        family, target = socket.AF_INET, ("127.0.0.1", int(address))
    else:
        family, target = socket.AF_UNIX, address
    deadline = time.monotonic() + timeout
    while True:
        connection = socket.socket(family)
        try:
            connection.connect(target)
            return connection
        except OSError:
            connection.close()
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.5)


class Qmp:
    def __init__(self, address):
        self.stream = connect(address).makefile("rw")
        self.stream.readline()  # 인사 메시지
        self.command("qmp_capabilities")
        self.buttons_ready = False

    def command(self, name, **arguments):
        message = {"execute": name}
        if arguments:
            message["arguments"] = arguments
        self.stream.write(json.dumps(message) + "\n")
        self.stream.flush()
        while True:
            reply = json.loads(self.stream.readline())
            if "return" in reply:
                return reply["return"]
            if "error" in reply:
                raise SystemExit(f"QMP {name}: {reply['error']}")

    def screen_size(self):
        # screendump 없이 크기를 알 방법이 없어 QEMU 기본 해상도를 쓴다
        return 1280, 800

    def absolute(self, x, y):
        """화면 픽셀 좌표를 QMP 절대 좌표로 바꾼다"""
        width, height = self.screen_size()
        return x * ABS_MAX // (width - 1), y * ABS_MAX // (height - 1)

    def move(self, x, y):
        x, y = self.absolute(x, y)
        events = [
            {"type": "abs", "data": {"axis": "x", "value": x}},
            {"type": "abs", "data": {"axis": "y", "value": y}},
        ]
        self.command("input-send-event", events=events)

    def ready_buttons(self):
        """단추·휠 이벤트가 가상 태블릿으로 가게 한다. 가상 터치 화면(virtio-multitouch)도 단추 이벤트를 받고, QEMU는
        단추를 받는 장치 가운데 게스트가 마지막에 켠 것에 보낸다. 그러면 누르기가 포인터 자리가 없는 터치 화면으로 간다.
        HMP의 mouse_set이 태블릿을 맨 앞으로 옮긴다. 터치 화면이 없어도 해가 없다"""
        if self.buttons_ready:
            return
        self.buttons_ready = True
        tablet = next((mouse for mouse in self.command("query-mice") if mouse.get("absolute")), None)
        if tablet is not None:
            self.command("human-monitor-command", **{"command-line": f"mouse_set {tablet['index']}"})

    def button(self, down, button="left"):
        self.ready_buttons()
        self.command("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": button}}])

    def click(self, x, y, count=1, button="left"):
        # 팝업 메뉴가 닫힌 직후의 첫 포인터 이동은 컴포지터(smithay)가 팝업 그랩을 푸는 데 쓰고
        # 버리므로, 두 번 옮겨 누르는 위치가 반영되게 한다
        self.move(x + 1, y)
        time.sleep(0.05)
        self.move(x, y)
        time.sleep(0.3)
        # 두 번 누르기로 인식되도록 누르는 간격을 짧게 둔다
        for _ in range(count):
            for down in (True, False):
                self.button(down, button)
                time.sleep(0.05)
        time.sleep(0.1)

    def drag(self, x1, y1, x2, y2, steps=12):
        self.move(x1, y1)
        time.sleep(0.3)
        self.button(True)
        time.sleep(0.1)
        for step in range(1, steps + 1):
            self.move(x1 + (x2 - x1) * step // steps, y1 + (y2 - y1) * step // steps)
            time.sleep(0.05)
        time.sleep(0.1)
        self.button(False)
        time.sleep(0.1)

    def keys(self, names):
        self.command("send-key", keys=[{"type": "qcode", "data": name} for name in names])

    def key(self, name, down):
        self.command("input-send-event", events=[{"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": name}}}])

    def wheel(self, direction, count):
        self.ready_buttons()
        # 휠 눈금 하나는 wheel-up/down 단추를 눌렀다 떼는 것이다 (QEMU 가상 태블릿의 REL_WHEEL)
        for _ in range(count):
            for down in (True, False):
                self.command("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": f"wheel-{direction}"}}])
            time.sleep(0.03)

    def finger(self, kind, x=0, y=0):
        """손가락 하나의 한 프레임을 보낸다: begin(누름)과 update(옮김)는 자리와 함께, end(뗌)는 손가락 번호를 지운다
        (-1). QMP의 input-send-event가 한 번에 보낸 이벤트 뒤에 SYN_REPORT를 하나 붙인다. 단추(BTN_TOUCH)는 보내지
        않는다: libinput은 멀티터치 화면의 손가락을 번호로 읽고, 단추 이벤트는 태블릿이 받게 둔다(ready_buttons).
        QEMU 8.1 이상의 mtt 이벤트다"""

        def event(kind, axis="x", value=0):
            tracking = -1 if kind == "end" else TOUCH_SLOT
            data = {"type": kind, "slot": TOUCH_SLOT, "tracking-id": tracking, "axis": axis, "value": value}
            return {"type": "mtt", "data": data}

        events = [event(kind)]
        if kind != "end":
            x, y = self.absolute(x, y)
            events += [event("data", "x", x), event("data", "y", y)]
        self.command("input-send-event", events=events)

    def touch(self, x, y):
        self.finger("begin", x, y)
        time.sleep(0.05)
        self.finger("end")
        time.sleep(0.1)

    def dark_pixels(self, path, x1, y1, x2, y2):
        """화면의 사각형에서 밝기가 어두운 점의 수. QEMU가 쓴 PPM(P6)을 읽는다"""
        self.command("screendump", filename=path)
        with open(path, "rb") as ppm:
            _, size, _, pixels = ppm.read().split(b"\n", 3)
        width = int(size.split()[0])
        count = 0
        for y in range(y1, y2):
            row = pixels[y * width * 3 + x1 * 3 : y * width * 3 + x2 * 3]
            count += sum(1 for i in range(0, len(row), 3) if row[i] + row[i + 1] + row[i + 2] < DARK)
        return count

    def tdrag(self, x1, y1, x2, y2, steps=12):
        # 한 번에 30ms씩 옮긴다. 가장자리 스와이프(comp/src/touch.rs)는 44px을 밀거나 빠르게 튕기면 알아본다
        steps = max(steps, 1)
        self.finger("begin", x1, y1)
        time.sleep(0.1)
        for step in range(1, steps + 1):
            self.finger("update", x1 + (x2 - x1) * step // steps, y1 + (y2 - y1) * step // steps)
            time.sleep(0.03)
        time.sleep(0.1)
        self.finger("end")
        time.sleep(0.1)


def main():
    qmp = Qmp(sys.argv[1])
    for action in sys.argv[2:]:
        kind, _, argument = action.partition(":")
        if kind == "overlay":
            qmp.command(
                "blockdev-snapshot-sync",
                **{"node-name": DISK_NODE, "snapshot-file": argument, "snapshot-node-name": "overlay", "format": "qcow2"},
            )
        elif kind == "cont":
            qmp.command("cont")
        elif kind in ("click", "dclick", "rclick", "move"):
            x, y = (int(value) for value in argument.split(","))
            if kind == "move":
                qmp.move(x, y)
            else:
                qmp.click(x, y, count=2 if kind == "dclick" else 1, button="right" if kind == "rclick" else "left")
        elif kind == "drag":
            qmp.drag(*(int(value) for value in argument.split(",")))
        elif kind in ("press", "release"):
            qmp.button(kind == "press")
        elif kind == "keys":
            qmp.keys(argument.split("+"))
        elif kind in ("down", "up"):
            qmp.key(argument, kind == "down")
        elif kind == "wheel":
            direction, _, count = argument.partition(",")
            qmp.wheel(direction, int(count or 1))
        elif kind == "touch":
            x, y = (int(value) for value in argument.split(","))
            qmp.touch(x, y)
        elif kind == "tdrag":
            values = [int(value) for value in argument.split(",")]
            if len(values) not in (4, 5):
                raise SystemExit(f"tdrag needs X1,Y1,X2,Y2[,STEPS]: {action}")
            qmp.tdrag(*values)
        elif kind == "type":
            for character in argument:
                qmp.keys(["spc" if character == " " else character])
                time.sleep(0.05)
        elif kind == "sleep":
            time.sleep(float(argument))
        elif kind == "screen":
            qmp.command("screendump", filename=argument, format="png")
        elif kind == "dark":
            path, *corners = argument.rsplit(",", 4)
            print(qmp.dark_pixels(path, *(int(value) for value in corners)))
        elif kind == "plug":
            driver, device, bus = argument.split(",")
            qmp.command("device_add", driver=driver, id=device, bus=bus)
        elif kind == "unplug":
            qmp.command("device_del", id=argument)
        else:
            raise SystemExit(f"unknown action: {action}")


if __name__ == "__main__":
    main()
