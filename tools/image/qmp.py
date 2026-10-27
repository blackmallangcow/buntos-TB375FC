# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""QEMU QMP로 VM에 입력을 보내고 화면을 저장한다 (ADR-009의 Phase 0 드라이버).

사용법: python3 qmp.py <포트 또는 소켓 경로> <동작>...
동작:
  overlay:PATH     디스크에 임시 qcow2 덮어쓰기(PATH)를 씌운다. 빌드한 이미지는 읽기만 한다
  cont             멈춘 VM을 이어서 실행한다 (QEMU를 -S로 띄웠을 때)
  click:X,Y        화면 좌표(X, Y)를 왼쪽 버튼으로 누른다 (가상 태블릿 필요)
  keys:A+B         키를 함께 누른다 (QEMU qcode 이름, 예: alt+ret)
  type:TEXT        영문 소문자·숫자를 한 글자씩 입력한다
  sleep:SECONDS    기다린다
  screen:PATH      화면을 PNG로 저장한다 (경로는 QEMU가 보는 경로)
"""

import json
import socket
import sys
import time

# QMP 절대 좌표 범위
ABS_MAX = 0x7FFF
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

    def click(self, x, y):
        width, height = self.screen_size()
        events = [
            {"type": "abs", "data": {"axis": "x", "value": x * ABS_MAX // (width - 1)}},
            {"type": "abs", "data": {"axis": "y", "value": y * ABS_MAX // (height - 1)}},
        ]
        self.command("input-send-event", events=events)
        time.sleep(0.3)
        for down in (True, False):
            self.command("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": "left"}}])
            time.sleep(0.1)

    def keys(self, names):
        self.command("send-key", keys=[{"type": "qcode", "data": name} for name in names])


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
        elif kind == "click":
            x, y = (int(value) for value in argument.split(","))
            qmp.click(x, y)
        elif kind == "keys":
            qmp.keys(argument.split("+"))
        elif kind == "type":
            for character in argument:
                qmp.keys(["spc" if character == " " else character])
                time.sleep(0.05)
        elif kind == "sleep":
            time.sleep(float(argument))
        elif kind == "screen":
            qmp.command("screendump", filename=argument, format="png")
        else:
            raise SystemExit(f"unknown action: {action}")


if __name__ == "__main__":
    main()
