// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 buntOS contributors
/*
 * Decode a VP9 file (IVF) through VA-API the way an FFmpeg-based player does,
 * and write the frames it would show as raw NV12.
 *
 * Like such a player it splits superframes, sends every frame that is decoded
 * (shown or not) as a picture of its own with a surface from a pool, keeps the
 * eight reference slots of VP9, and shows a frame again without telling the
 * driver when the stream says show_existing_frame.
 *
 * va-vp9-test [-d render node] [-g] [-n] [-p pool] file.ivf [out.nv12]
 *   -g  read frames with vaGetImage instead of the exported dma-buf
 *   -n  take the frames and do not read them (speed of the decoding alone)
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/mman.h>
#include <va/va.h>
#include <va/va_drm.h>
#include <va/va_str.h>
#include <va/va_drmcommon.h>

#define MAX_POOL 32

static VADisplay dpy;
static VAContextID context;
static VASurfaceID pool[MAX_POOL];
static int npool = 12;
static int slots[8] = { -1, -1, -1, -1, -1, -1, -1, -1 };
static int width, height;
static bool use_getimage, no_read;
static FILE *out;
static unsigned long shown, decoded, hidden, reshown, failed;

#define CHECK(call) do { \
	VAStatus st_ = (call); \
	if (st_ != VA_STATUS_SUCCESS) { \
		fprintf(stderr, "%s: %s\n", #call, vaErrorStr(st_)); \
		exit(1); \
	} } while (0)

struct head {
	bool show_existing, show, key;
	int slot, refresh;
};

static int parse(const uint8_t *p, size_t len, struct head *h)
{
	unsigned int pos = 0;
#define BITS(n) ({ \
	unsigned int v_ = 0, n_ = (n); \
	while (n_--) { \
		if (pos / 8 >= len) \
			return -1; \
		v_ = v_ << 1 | ((p[pos / 8] >> (7 - pos % 8)) & 1); \
		pos++; \
	} \
	v_; })
	bool error_res, intra_only = false;
	int profile;

	memset(h, 0, sizeof(*h));
	if (BITS(2) != 2)
		return -1;
	profile = BITS(1);
	profile |= BITS(1) << 1;
	if (profile)
		return -1;
	h->show_existing = BITS(1);
	if (h->show_existing) {
		h->slot = BITS(3);
		return 0;
	}
	h->key = !BITS(1);
	h->show = BITS(1);
	error_res = BITS(1);
	if (h->key) {
		h->refresh = 0xff;
		return 0;
	}
	if (!h->show)
		intra_only = BITS(1);
	if (!error_res)
		(void)BITS(2);
	if (intra_only && BITS(24) != 0x498342)
		return -1;
	h->refresh = BITS(8);
	return 0;
#undef BITS
}

/* What a player does with a frame it shows */
static void show(int surface)
{
	static uint8_t *line;
	VADRMPRIMESurfaceDescriptor desc;
	VAStatus st;
	int y;

	shown++;
	if (use_getimage) {
		static VAImage image;
		VAImageFormat fmt = { .fourcc = VA_FOURCC_NV12, .byte_order = VA_LSB_FIRST, .bits_per_pixel = 12 };
		uint8_t *data;

		if (!image.buf)
			CHECK(vaCreateImage(dpy, &fmt, width, height, &image));
		st = vaGetImage(dpy, pool[surface], 0, 0, width, height, image.image_id);
		if (st != VA_STATUS_SUCCESS) {
			fprintf(stderr, "frame %lu: vaGetImage: %s\n", shown - 1, vaErrorStr(st));
			failed++;
			return;
		}
		CHECK(vaMapBuffer(dpy, image.buf, (void **)&data));
		for (y = 0; out && y < height; y++)
			fwrite(data + image.offsets[0] + (size_t)y * image.pitches[0], 1, width, out);
		for (y = 0; out && y < height / 2; y++)
			fwrite(data + image.offsets[1] + (size_t)y * image.pitches[1], 1, width, out);
		vaUnmapBuffer(dpy, image.buf);
		return;
	}

	st = vaExportSurfaceHandle(dpy, pool[surface], VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2,
				   VA_EXPORT_SURFACE_READ_ONLY | VA_EXPORT_SURFACE_SEPARATE_LAYERS, &desc);
	if (st != VA_STATUS_SUCCESS) {
		fprintf(stderr, "frame %lu: vaExportSurfaceHandle: %s\n", shown - 1, vaErrorStr(st));
		failed++;
		return;
	}
	st = vaSyncSurface(dpy, pool[surface]);
	if (st != VA_STATUS_SUCCESS)
		fprintf(stderr, "frame %lu: vaSyncSurface: %s\n", shown - 1, vaErrorStr(st));
	if (shown == 1)
		fprintf(stderr, "first frame: %ux%u, %u objects, %u layers, pitch %u, chroma at %u, %u bytes\n",
			desc.width, desc.height, desc.num_objects, desc.num_layers, desc.layers[0].pitch[0],
			desc.layers[1].offset[0], desc.objects[0].size);
	if (out && !no_read) {
		const uint8_t *map = mmap(NULL, desc.objects[0].size, PROT_READ, MAP_SHARED, desc.objects[0].fd, 0);
		uint32_t pitch = desc.layers[0].pitch[0], chroma = desc.layers[1].offset[0];

		if (map == MAP_FAILED) {
			perror("mmap of the frame");
			exit(1);
		}
		if (!line)
			line = malloc(width);
		/* One line at a time through ordinary memory: the frame is not cached memory */
		for (y = 0; y < height; y++) {
			memcpy(line, map + (size_t)y * pitch, width);
			fwrite(line, 1, width, out);
		}
		for (y = 0; y < height / 2; y++) {
			memcpy(line, map + chroma + (size_t)y * desc.layers[1].pitch[0], width);
			fwrite(line, 1, width, out);
		}
		munmap((void *)map, desc.objects[0].size);
	}
	close(desc.objects[0].fd);
}

static int free_surface(void)
{
	int i, k;

	for (i = 0; i < npool; i++) {
		for (k = 0; k < 8 && slots[k] != i; k++)
			;
		if (k == 8)
			return i;
	}
	fprintf(stderr, "no free surface\n");
	exit(1);
}

static void frame(const uint8_t *data, size_t len)
{
	VADecPictureParameterBufferVP9 pic = { .frame_width = width, .frame_height = height };
	VASliceParameterBufferVP9 slice = { .slice_data_size = len, .slice_data_flag = VA_SLICE_DATA_FLAG_ALL };
	VABufferID bufs[3];
	struct head h;
	VAStatus st;
	int s, k;

	if (!len)
		return;
	if (parse(data, len, &h) < 0) {
		fprintf(stderr, "a frame of %zu bytes that is not VP9 profile 0\n", len);
		exit(1);
	}
	if (h.show_existing) {
		reshown++;
		if (slots[h.slot] < 0) {
			fprintf(stderr, "show_existing_frame of an empty slot\n");
			exit(1);
		}
		show(slots[h.slot]);
		return;
	}
	decoded++;
	hidden += !h.show;
	s = free_surface();
	pic.pic_fields.bits.show_frame = h.show;
	pic.pic_fields.bits.frame_type = !h.key;
	CHECK(vaCreateBuffer(dpy, context, VAPictureParameterBufferType, sizeof(pic), 1, &pic, &bufs[0]));
	CHECK(vaCreateBuffer(dpy, context, VASliceParameterBufferType, sizeof(slice), 1, &slice, &bufs[1]));
	CHECK(vaCreateBuffer(dpy, context, VASliceDataBufferType, len, 1, (void *)data, &bufs[2]));
	CHECK(vaBeginPicture(dpy, context, pool[s]));
	CHECK(vaRenderPicture(dpy, context, bufs, 3));
	st = vaEndPicture(dpy, context);
	if (st != VA_STATUS_SUCCESS) {
		fprintf(stderr, "picture %lu: vaEndPicture: %s\n", decoded - 1, vaErrorStr(st));
		failed++;
	}
	for (k = 0; k < 3; k++)
		vaDestroyBuffer(dpy, bufs[k]);
	for (k = 0; k < 8; k++)
		if (h.refresh & (1 << k))
			slots[k] = s;
	if (h.show)
		show(s);
}

/* A packet holds one frame, or several with a list of their sizes at the end */
static void packet(const uint8_t *data, size_t len)
{
	uint8_t marker = len ? data[len - 1] : 0;

	if ((marker & 0xe0) == 0xc0) {
		unsigned int frames = (marker & 7) + 1, mag = ((marker >> 3) & 3) + 1, i, k;
		size_t index = 2 + mag * frames;

		if (len >= index && data[len - index] == marker) {
			const uint8_t *x = data + len - index + 1;

			for (i = 0; i < frames; i++) {
				size_t size = 0;

				for (k = 0; k < mag; k++)
					size |= (size_t)*x++ << (8 * k);
				if (size > len - index) {
					fprintf(stderr, "a broken superframe\n");
					exit(1);
				}
				frame(data, size);
				data += size;
				len -= size;
			}
			return;
		}
	}
	frame(data, len);
}

int main(int argc, char **argv)
{
	const char *node = "/dev/dri/renderD128";
	VAConfigID config;
	uint8_t head[32], fh[12], *buf = NULL;
	size_t cap = 0;
	struct timespec t0, t1;
	int major, minor, fd, opt;
	double secs;
	FILE *in;

	while ((opt = getopt(argc, argv, "d:gnp:")) != -1) {
		switch (opt) {
		case 'd':
			node = optarg;
			break;
		case 'g':
			use_getimage = true;
			break;
		case 'n':
			no_read = true;
			break;
		case 'p':
			npool = atoi(optarg);
			break;
		default:
			return 2;
		}
	}
	if (optind >= argc || npool < 10 || npool > MAX_POOL) {
		fprintf(stderr, "usage: %s [-d render node] [-g] [-n] [-p pool 10..%d] file.ivf [out.nv12]\n",
			argv[0], MAX_POOL);
		return 2;
	}
	in = fopen(argv[optind], "rb");
	if (!in || fread(head, 1, 32, in) != 32 || memcmp(head, "DKIF", 4) || memcmp(head + 8, "VP90", 4)) {
		fprintf(stderr, "%s: not a VP9 IVF file\n", argv[optind]);
		return 1;
	}
	width = head[12] | head[13] << 8;
	height = head[14] | head[15] << 8;
	if (optind + 1 < argc) {
		out = strcmp(argv[optind + 1], "-") ? fopen(argv[optind + 1], "wb") : stdout;
		if (!out) {
			perror(argv[optind + 1]);
			return 1;
		}
		setvbuf(out, NULL, _IOFBF, 4 << 20);
	}

	fd = open(node, O_RDWR | O_CLOEXEC);
	if (fd < 0) {
		perror(node);
		return 1;
	}
	dpy = vaGetDisplayDRM(fd);
	CHECK(vaInitialize(dpy, &major, &minor));
	fprintf(stderr, "%s, %dx%d\n", vaQueryVendorString(dpy), width, height);
	{
		VAProfile profiles[64];
		int n = 0, i;

		if (vaMaxNumProfiles(dpy) <= 64 && vaQueryConfigProfiles(dpy, profiles, &n) == VA_STATUS_SUCCESS)
			for (i = 0; i < n; i++)
				fprintf(stderr, "profile: %s\n", vaProfileStr(profiles[i]));
	}
	CHECK(vaCreateConfig(dpy, VAProfileVP9Profile0, VAEntrypointVLD, NULL, 0, &config));
	CHECK(vaCreateSurfaces(dpy, VA_RT_FORMAT_YUV420, width, height, pool, npool, NULL, 0));
	CHECK(vaCreateContext(dpy, config, width, height, VA_PROGRESSIVE, pool, npool, &context));

	clock_gettime(CLOCK_MONOTONIC, &t0);
	while (fread(fh, 1, 12, in) == 12) {
		size_t len = fh[0] | fh[1] << 8 | fh[2] << 16 | (size_t)fh[3] << 24;

		if (len > cap) {
			cap = len * 2;
			buf = realloc(buf, cap);
		}
		if (!buf || fread(buf, 1, len, in) != len)
			break;
		packet(buf, len);
	}
	clock_gettime(CLOCK_MONOTONIC, &t1);
	secs = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) / 1e9;
	fprintf(stderr, "shown %lu (again %lu), decoded %lu (not shown %lu), failed %lu, %.2f s, %.1f frames/s\n",
		shown, reshown, decoded, hidden, failed, secs, shown / secs);

	vaDestroyContext(dpy, context);
	vaDestroySurfaces(dpy, pool, npool);
	vaDestroyConfig(dpy, config);
	vaTerminate(dpy);
	if (out)
		fclose(out);
	return failed ? 1 : 0;
}
