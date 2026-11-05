#!/usr/bin/python3 -I
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""e2e 10번의 작은 Wayland 클라이언트 (tests/e2e/clipboard.sh). 이미지에 없는 것을 대신한다.

wl-copy는 형식을 하나만 내놓아 비밀번호 관리자의 복사(글과 x-kde-passwordManagerHint)를 흉내 내지 못하고,
Flatpak 앱을 띄우려면 런타임을 받아야 한다. 그래서 표준 라이브러리만으로 Wayland 메시지를 직접 쓴다.

사용법:
  wayland-probe.py globals            이 연결에 클립보드 프로토콜과 보안 컨텍스트가 보이는지
                                      (예: ext=1 zwlr=1 security=1)
  wayland-probe.py sandbox-globals    보안 컨텍스트로 만든 소켓에 붙어 같은 것을 본다
  wayland-probe.py copy-secret TEXT   비밀번호 관리자처럼 복사하고, 다른 복사가 올 때까지 내용을 내준다
"""

import array
import os
import socket
import struct
import sys
import tempfile

DISPLAY = 1
# wl_display
GET_REGISTRY = 1
SYNC = 0
# wp_security_context_manager_v1, wp_security_context_v1
CREATE_LISTENER = 1
SET_SANDBOX_ENGINE = 1
COMMIT = 4
# ext_data_control_manager_v1, ext_data_control_source_v1, ext_data_control_device_v1
CREATE_DATA_SOURCE = 0
GET_DATA_DEVICE = 1
SOURCE_OFFER = 0
EVENT_SEND = 0
EVENT_CANCELLED = 1
SET_SELECTION = 0

SECURITY = "wp_security_context_manager_v1"
EXT = "ext_data_control_manager_v1"
ZWLR = "zwlr_data_control_manager_v1"
SECRET_MIMES = ["text/plain;charset=utf-8", "text/plain", "x-kde-passwordManagerHint"]


def wayland_string(text):
    data = text.encode() + b"\0"
    return struct.pack("<I", len(data)) + data + b"\0" * (-len(data) % 4)


def socket_path():
    return os.path.join(os.environ["XDG_RUNTIME_DIR"], os.environ["WAYLAND_DISPLAY"])


class Client:
    def __init__(self, path):
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.socket.connect(path)
        self.next_id = 2
        self.buffer = b""
        self.fds = []
        self.globals = {}
        self.registry = self.new_id()
        self.send(DISPLAY, GET_REGISTRY, struct.pack("<I", self.registry))
        self.sync()

    def new_id(self):
        self.next_id += 1
        return self.next_id - 1

    def send(self, target, opcode, payload=b"", fds=()):
        size = 8 + len(payload)
        message = struct.pack("<II", target, size << 16 | opcode) + payload
        extra = []
        if fds:
            extra = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", fds))]
        self.socket.sendmsg([message], extra)

    def events(self):
        """받은 메시지를 하나씩 내놓는다: 보낸 객체, 번호, 내용"""
        while True:
            while len(self.buffer) >= 8:
                target, word = struct.unpack_from("<II", self.buffer)
                size, opcode = word >> 16, word & 0xFFFF
                if len(self.buffer) < size:
                    break
                body = self.buffer[8:size]
                self.buffer = self.buffer[size:]
                yield target, opcode, body
            data, ancillary, _, _ = self.socket.recvmsg(4096, socket.CMSG_SPACE(32))
            if not data:
                sys.exit("the compositor closed the connection")
            self.buffer += data
            for level, kind, payload in ancillary:
                if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                    fds = array.array("i")
                    fds.frombytes(payload[: len(payload) - len(payload) % fds.itemsize])
                    self.fds.extend(fds)

    def sync(self):
        """보낸 요청이 다 처리되고 그 답이 올 때까지 읽는다. 전역 목록도 여기서 모은다"""
        callback = self.new_id()
        self.send(DISPLAY, SYNC, struct.pack("<I", callback))
        for target, opcode, body in self.events():
            if target == self.registry and opcode == 0:
                name, length = struct.unpack_from("<II", body)
                interface = body[8 : 8 + length - 1].decode()
                self.globals[interface] = name
            elif target == DISPLAY and opcode == 0:
                sys.exit(f"protocol error: {body!r}")
            elif target == callback:
                return

    def bind(self, interface, version=1):
        object_id = self.new_id()
        self.send(
            self.registry,
            0,
            struct.pack("<I", self.globals[interface])
            + wayland_string(interface)
            + struct.pack("<II", version, object_id),
        )
        return object_id


def seen(client):
    has = lambda interface: int(interface in client.globals)
    return f"ext={has(EXT)} zwlr={has(ZWLR)} security={has(SECURITY)}"


def sandbox_globals():
    """Flatpak과 같은 길: 소켓을 만들어 넘기고 앱은 그 소켓에 붙는다"""
    client = Client(socket_path())
    manager = client.bind(SECURITY)
    directory = tempfile.mkdtemp(dir=os.environ["XDG_RUNTIME_DIR"])
    path = os.path.join(directory, "sandbox")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(path)
    listener.listen()
    close_read, close_write = os.pipe()
    context = client.new_id()
    client.send(
        manager,
        CREATE_LISTENER,
        struct.pack("<I", context),
        [listener.fileno(), close_read],
    )
    client.send(context, SET_SANDBOX_ENGINE, wayland_string("org.buntos.e2e"))
    client.send(context, COMMIT)
    client.sync()
    answer = seen(Client(path))
    os.close(close_write)
    os.unlink(path)
    os.rmdir(directory)
    return answer


def copy_secret(text):
    client = Client(socket_path())
    manager = client.bind(EXT)
    seat = client.bind("wl_seat")
    source = client.new_id()
    client.send(manager, CREATE_DATA_SOURCE, struct.pack("<I", source))
    for mime in SECRET_MIMES:
        client.send(source, SOURCE_OFFER, wayland_string(mime))
    device = client.new_id()
    client.send(manager, GET_DATA_DEVICE, struct.pack("<II", device, seat))
    client.send(device, SET_SELECTION, struct.pack("<I", source))
    client.sync()
    # 복사가 끝났다고 알리고 내용을 내주러 뒤로 간다
    if os.fork():
        print("copied", flush=True)
        os._exit(0)
    devnull = os.open(os.devnull, os.O_RDWR)
    for fd in (0, 1, 2):
        os.dup2(devnull, fd)
    for target, opcode, body in client.events():
        if target != source:
            continue
        if opcode == EVENT_CANCELLED:
            return
        if opcode == EVENT_SEND:
            length = struct.unpack_from("<I", body)[0]
            mime = body[4 : 4 + length - 1].decode()
            fd = client.fds.pop(0)
            data = b"secret" if mime == SECRET_MIMES[-1] else text.encode()
            os.write(fd, data)
            os.close(fd)


def main():
    command = sys.argv[1]
    if command == "globals":
        print(seen(Client(socket_path())))
    elif command == "sandbox-globals":
        print(sandbox_globals())
    elif command == "copy-secret":
        copy_secret(sys.argv[2])
    else:
        sys.exit(__doc__)


main()
