#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# 작업 트리의 setup.c를 고쳐 커널에 넣은 트리를 쓰게 한다(시험용). build.sh prepare가 되돌린다.
import sys
t = sys.argv[1]; dtb = sys.argv[2]
p = t + '/arch/arm64/kernel/setup.c'
s = open(p).read()
old1 = '''static void __init setup_machine_fdt(phys_addr_t dt_phys)
{
	int size = 0;
	void *dt_virt = fixmap_remap_fdt(dt_phys, &size, PAGE_KERNEL);
	const char *name;

	if (dt_virt)
		memblock_reserve(dt_phys, size);

'''
new1 = '''extern const char __tb375fc_dtb[];

static void __init setup_machine_fdt(phys_addr_t dt_phys)
{
	int size = 0;
	void *dt_virt = (void *)__tb375fc_dtb;
	const char *name;

	/* Test kernel: ignore the bootloader's tree and use the built-in one */
	dt_phys = __pa_symbol(__tb375fc_dtb);

'''
old2 = '''	/* Early fixups are done, map the FDT as read-only now */
	fixmap_remap_fdt(dt_phys, &size, PAGE_KERNEL_RO);

'''
assert s.count(old1) == 1 and s.count(old2) == 1
s = s.replace(old1, new1).replace(old2, '')
open(p, 'w').write(s)
# REUSE-IgnoreStart
open(t + '/arch/arm64/kernel/tb375fc-dtb.S', 'w').write(f'''/* SPDX-License-Identifier: GPL-2.0 */
	.section .rodata, "a"
	.balign 8
	.globl __tb375fc_dtb
__tb375fc_dtb:
	.incbin "{dtb}"
''')
# REUSE-IgnoreEnd
m = t + '/arch/arm64/kernel/Makefile'
ms = open(m).read()
if 'tb375fc-dtb.o' not in ms:
    ms += '\nobj-y += tb375fc-dtb.o\n'
    open(m, 'w').write(ms)
print('적용')
