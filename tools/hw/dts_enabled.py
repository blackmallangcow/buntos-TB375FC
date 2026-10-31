# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""dtc가 푼 DTS에서 켜진 노드(자기와 조상의 status가 없거나 okay)의 경로와 compatible을 탭으로 나눠 낸다.
보드 오버레이를 입힌 트리(fdtoverlay)에 쓴다.

사용법: dts_enabled.py <파일.dts>
"""
import re, sys
stack, nodes = [], []
cur = None
for line in open(sys.argv[1], encoding="utf-8", errors="replace"):
    s = line.strip()
    m = re.match(r'^(?:[\w,.\-]+:\s*)?([\w,.\-@/]+)\s*\{$', s)
    if m:
        cur = {"path": "/".join(n["name"] for n in stack) + "/" + m.group(1), "name": m.group(1), "compat": None, "status": None}
        stack.append(cur); nodes.append(cur); continue
    if s == "};":
        if stack: stack.pop()
        continue
    if stack:
        if s.startswith("compatible ="):
            stack[-1]["compat"] = re.findall(r'"([^"]*)"', s)
        elif s.startswith("status ="):
            stack[-1]["status"] = re.findall(r'"([^"]*)"', s)[0]
def enabled(node_index):
    # 조상이 꺼져 있으면 꺼진 것이다
    path = nodes[node_index]["path"]
    for n in nodes:
        if path.startswith(n["path"] + "/") or n is nodes[node_index]:
            if n["status"] not in (None, "okay", "ok"):
                return False
    return True
for i, n in enumerate(nodes):
    if n["compat"] and enabled(i):
        print(f'{n["path"].lstrip("/")}\t{", ".join(n["compat"])}')
