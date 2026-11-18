# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 buntOS contributors
#
# The VA-API driver for the MT6897 video decoder, and its test program.
# Needs libva-dev and the kernel headers (linux-libc-dev).

CC ?= cc
CFLAGS ?= -O2 -g
WARN = -Wall -Wextra -Werror
VA_CFLAGS ?= $(shell pkg-config --cflags libva)
VA_LIBS ?= $(shell pkg-config --libs libva libva-drm)

all: mt6897_vdec_drv_video.so va-vp9-test

mt6897_vdec_drv_video.so: mt6897_va.c
	$(CC) $(CFLAGS) $(WARN) $(VA_CFLAGS) -fPIC -fvisibility=hidden -shared -o $@ $< -lpthread

va-vp9-test: va-vp9-test.c
	$(CC) $(CFLAGS) $(WARN) $(VA_CFLAGS) -o $@ $< $(VA_LIBS)

clean:
	rm -f mt6897_vdec_drv_video.so va-vp9-test

.PHONY: all clean
