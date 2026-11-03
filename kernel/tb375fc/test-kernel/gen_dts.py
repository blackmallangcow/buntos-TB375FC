#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# 기기의 살아 있는 트리에서 고정 예약 영역을 읽어 시험용 dts를 만든다.
# 커널에 넣는 트리는 LK가 더하는 값을 받지 못하므로 그 값을 여기에 적는다(8GiB 기기, 16.0.20.091 LK).
# 사용법: gen_dts.py <안드로이드의 /sys/firmware/fdt> <출력 dts>. 원본 트리에는 식별자가 있어 저장소에 넣지 않는다.
import struct, sys
b = open(sys.argv[1], 'rb').read()
_, _, off_struct, off_str = struct.unpack('>4I', b[:16])
def s(off):
    e = b.index(b'\0', off_str + off); return b[off_str + off:e].decode()
p = off_struct; path = []; nodes = {}
while True:
    tok = struct.unpack('>I', b[p:p+4])[0]; p += 4
    if tok == 1:
        e = b.index(b'\0', p); name = b[p:e].decode(); p = (e + 4) & ~3; path.append(name)
        nodes['/'.join(path)] = {}
    elif tok == 3:
        l, n = struct.unpack('>II', b[p:p+8]); p += 8
        nodes['/'.join(path)][s(n)] = b[p:p+l]; p = (p + l + 3) & ~3
    elif tok == 2: path.pop()
    elif tok == 9: break
rows = []
for n, v in nodes.items():
    if not n.startswith('/reserved-memory/'): continue
    r = v.get('reg')
    if not r or len(r) != 16: continue
    name = n.split('/')[-1]
    if name in ('mblock-14-framebuffer', 'mblock-2-pstore'):  # 보드 dts의 framebuffer_mem, ramoops가 같은 자리다
        continue
    a, l = struct.unpack('>QQ', r)
    rows.append((a, l, name, 'no-map' in v))
rows.sort()
out = ['// SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)', '// 시험용: 커널에 넣는 트리. gen_dts.py가 기기의 살아 있는 트리에서 만들었다.',
       '#include "mt6897-lenovo-tb375fc.dts"', '#include "test-stage.dtsi"', '', '/ {', '\tchosen {', '\t\t/delete-property/ stdout-path;',
       '\t\tbootargs = "console=tty0 ignore_loglevel log_buf_len=1M root=PARTLABEL=buntos rootfstype=btrfs rw systemd.firstboot=no buntos.usbdebug=1 buntos.wait=10 buntos.telnet=1 buntos.autoreboot=30 buntos.dump=1 panic=30 clk_ignore_unused pd_ignore_unused regulator_ignore_unused arm64.nomte";',
       '\t};', '', '\tmemory@40000000 {', '\t\treg = <0 0x40000000 0x2 0x0>;', '\t};', '};', '',
       '&uart0 {', '\tstatus = "disabled";', '};', '', '&reserved_memory {']
for a, l, name, nm in rows:
    label = name.replace('_', '-').lower()
    out += [f'\t{label}@{a:x} {{', f'\t\treg = <{a >> 32:#x} {a & 0xffffffff:#x} {l >> 32:#x} {l & 0xffffffff:#x}>;']
    if nm: out.append('\t\tno-map;')
    out.append('\t};')
out.append('};')
open(sys.argv[2], 'w').write('\n'.join(out) + '\n')
print('예약 항목', len(rows))
