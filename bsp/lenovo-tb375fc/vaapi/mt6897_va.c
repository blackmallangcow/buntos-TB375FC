// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 buntOS contributors
/*
 * VA-API on top of the stateful V4L2 decoder of the MT6897 (mt6897-vdec).
 *
 * The decoder reads the stream itself, so a picture is just the bytes of its
 * slices, queued as one buffer. Frames come back as plain NV12 in capture
 * buffers, in the order of display. A surface is whichever capture buffer its
 * picture arrived in, and it is handed to the application as a dma-buf: no
 * pixel is copied here.
 *
 * VA-API works in the order of decoding and wants every picture in the surface
 * the application named. Pictures are matched to frames by a number carried in
 * the timestamp of the buffers.
 *
 * VP9 has frames that are decoded and not shown, which an application may show
 * later without telling the driver (show_existing_frame). The decoder never
 * puts such a frame out by itself, so right after it the driver sends the
 * one-byte frame that shows it. That frame changes nothing in the state of a
 * VP9 decoder.
 *
 * The structure follows vaapi-mtk-vcp of the MT6895-Mainline project (MIT),
 * which does the same for the sibling SoC and its own kernel driver.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <drm/drm_fourcc.h>
#include <linux/videodev2.h>
#include <va/va.h>
#include <va/va_backend.h>
#include <va/va_drmcommon.h>

#define DECODER_NAME	"mt6897-vdec"
#define MAX_W		4096
#define MAX_H		2176
#define MIN_W		64
#define MIN_H		64

#define N_CONFIG	16
#define N_CONTEXT	4
#define N_SURFACE	128
#define N_BUFFER	1024
#define N_IMAGE		16
#define N_OUT		6	/* buffers for the stream */
#define N_CAP		32	/* buffers for frames, at most */
#define N_CAP_FIRST	4	/* more are made when pictures wait for one */

#define ID_CONFIG	0x01000000u
#define ID_CONTEXT	0x02000000u
#define ID_SURFACE	0x04000000u
#define ID_BUFFER	0x08000000u
#define ID_IMAGE	0x10000000u

#define HEADER_WAIT_MS	5000
#define SUBMIT_WAIT_MS	4000
#define EXPORT_WAIT_MS	2000
#define CLOSE_WAIT_MS	100

enum { S_EMPTY, S_PENDING, S_READY, S_FAILED };

struct config {
	bool used;
	VAProfile profile;
};

struct surface {
	bool used;
	int width, height;
	int state;
	int ctx;		/* the context that decodes into it, or -1 */
	unsigned int ctx_gen;
	int cap;		/* its capture buffer in that context, or -1 */
	int fd;			/* the frame as a dma-buf, or -1 */
	uint32_t stride, lines, size;
	uint64_t cookie;
};

struct buffer {
	bool used;
	VABufferType type;
	unsigned int size, count;
	void *data;
};

struct image {
	bool used;
	VAImage va;
};

struct context {
	bool used;
	unsigned int gen;
	int fd;
	int width, height;
	int nout;
	void *out_map[N_OUT];
	size_t out_len;
	bool out_busy[N_OUT];
	bool capturing;
	struct v4l2_format cap_fmt;
	int ncap, queued;
	int cap_fd[N_CAP];
	uint32_t stride, lines, size;
	uint64_t seq;
	int target;		/* the surface of the picture being put together, or -1 */
	uint8_t *au;
	size_t au_len, au_cap;
	bool failed;
};

struct driver {
	pthread_mutex_t lock;
	char node[32];
	unsigned int gen;
	struct config configs[N_CONFIG];
	struct context contexts[N_CONTEXT];
	struct surface surfaces[N_SURFACE];
	struct buffer buffers[N_BUFFER];
	struct image images[N_IMAGE];
};

static bool debug_on(void)
{
	static int on = -1;

	if (on < 0) {
		const char *e = getenv("MT6897_VA_DEBUG");

		on = e && *e && *e != '0';
	}
	return on;
}

__attribute__((format(printf, 1, 2)))
static void dbg(const char *fmt, ...)
{
	va_list ap;

	if (!debug_on())
		return;
	fputs("mt6897-va: ", stderr);
	va_start(ap, fmt);
	vfprintf(stderr, fmt, ap);
	va_end(ap);
	fputc('\n', stderr);
}

static uint64_t now_ms(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (uint64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static int xioctl(int fd, unsigned long req, void *arg)
{
	int r;

	do {
		r = ioctl(fd, req, arg);
	} while (r < 0 && errno == EINTR);
	return r;
}

static struct driver *drv(VADriverContextP ctx)
{
	return ctx->pDriverData;
}

/* The decoder among the video devices: the number of the node is not fixed */
static int find_node(char *node, size_t len)
{
	const char *env = getenv("MT6897_VA_NODE");
	int i;

	if (env && *env) {
		snprintf(node, len, "%s", env);
		return 0;
	}
	for (i = 0; i < 64; i++) {
		struct v4l2_capability cap;
		int fd, r;

		snprintf(node, len, "/dev/video%d", i);
		fd = open(node, O_RDWR | O_NONBLOCK | O_CLOEXEC);
		if (fd < 0)
			continue;
		r = xioctl(fd, VIDIOC_QUERYCAP, &cap);
		close(fd);
		if (!r && !strcmp((const char *)cap.driver, DECODER_NAME))
			return 0;
	}
	return -1;
}

/* Handles */

static struct config *get_config(struct driver *d, VAConfigID id)
{
	unsigned int i = id - ID_CONFIG;

	return i < N_CONFIG && d->configs[i].used ? &d->configs[i] : NULL;
}

static struct context *get_context(struct driver *d, VAContextID id)
{
	unsigned int i = id - ID_CONTEXT;

	return i < N_CONTEXT && d->contexts[i].used ? &d->contexts[i] : NULL;
}

static struct surface *get_surface(struct driver *d, VASurfaceID id)
{
	unsigned int i = id - ID_SURFACE;

	return i < N_SURFACE && d->surfaces[i].used ? &d->surfaces[i] : NULL;
}

static struct buffer *get_buffer(struct driver *d, VABufferID id)
{
	unsigned int i = id - ID_BUFFER;

	return i < N_BUFFER && d->buffers[i].used ? &d->buffers[i] : NULL;
}

static struct image *get_image(struct driver *d, VAImageID id)
{
	unsigned int i = id - ID_IMAGE;

	return i < N_IMAGE && d->images[i].used ? &d->images[i] : NULL;
}

/* The context a surface still decodes into, if that context is alive */
static struct context *surface_context(struct driver *d, struct surface *s)
{
	struct context *c;

	if (s->ctx < 0)
		return NULL;
	c = &d->contexts[s->ctx];
	return c->used && c->gen == s->ctx_gen ? c : NULL;
}

/* The decoder */

static int queue_cap(struct context *c, int i)
{
	struct v4l2_plane pl = { 0 };
	struct v4l2_buffer b = {
		.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, .memory = V4L2_MEMORY_MMAP,
		.index = i, .length = 1, .m.planes = &pl,
	};

	if (xioctl(c->fd, VIDIOC_QBUF, &b) < 0) {
		dbg("queue frame buffer %d: %s", i, strerror(errno));
		return -1;
	}
	c->queued++;
	return 0;
}

static int export_cap(struct context *c, int i)
{
	struct v4l2_exportbuffer e = {
		.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, .index = i, .flags = O_RDONLY | O_CLOEXEC,
	};

	if (xioctl(c->fd, VIDIOC_EXPBUF, &e) < 0) {
		dbg("export frame buffer %d: %s", i, strerror(errno));
		return -1;
	}
	c->cap_fd[i] = e.fd;
	return 0;
}

/* One more frame buffer, for when every one is held by a surface */
static int add_cap(struct context *c)
{
	struct v4l2_create_buffers cb = {
		.count = 1, .memory = V4L2_MEMORY_MMAP, .format = c->cap_fmt,
	};

	if (c->ncap >= N_CAP)
		return -1;
	if (xioctl(c->fd, VIDIOC_CREATE_BUFS, &cb) < 0 || cb.count < 1 || (int)cb.index != c->ncap) {
		dbg("one more frame buffer: %s", strerror(errno));
		return -1;
	}
	if (export_cap(c, c->ncap) < 0)
		return -1;
	c->ncap++;
	return queue_cap(c, c->ncap - 1);
}

/* The decoder has read the headers: it now says what its frames look like */
static int start_capture(struct context *c)
{
	struct v4l2_format *f = &c->cap_fmt;
	struct v4l2_requestbuffers rb = {
		.count = N_CAP_FIRST, .type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, .memory = V4L2_MEMORY_MMAP,
	};
	int type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
	unsigned int i;

	memset(f, 0, sizeof(*f));
	f->type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
	if (xioctl(c->fd, VIDIOC_G_FMT, f) < 0)
		return -1;
	if (f->fmt.pix_mp.pixelformat != V4L2_PIX_FMT_NV12 || f->fmt.pix_mp.num_planes != 1) {
		dbg("frames are not plain NV12");
		return -1;
	}
	c->stride = f->fmt.pix_mp.plane_fmt[0].bytesperline;
	c->lines = f->fmt.pix_mp.height;
	c->size = f->fmt.pix_mp.plane_fmt[0].sizeimage;
	if (c->stride < (uint32_t)c->width || c->lines < (uint32_t)c->height ||
	    (uint64_t)c->stride * c->lines * 3 / 2 > c->size) {
		dbg("frames of %ux%u in %u bytes do not hold %dx%d", c->stride, c->lines, c->size,
		    c->width, c->height);
		return -1;
	}
	if (xioctl(c->fd, VIDIOC_REQBUFS, &rb) < 0 || rb.count < 1)
		return -1;
	for (i = 0; i < rb.count && i < N_CAP; i++) {
		if (export_cap(c, i) < 0)
			return -1;
		c->ncap = i + 1;
		if (queue_cap(c, i) < 0)
			return -1;
	}
	if (xioctl(c->fd, VIDIOC_STREAMON, &type) < 0) {
		dbg("start of frames: %s", strerror(errno));
		return -1;
	}
	c->capturing = true;
	dbg("frames %ux%u, %u bytes, %d buffers", c->stride, c->lines, c->size, c->ncap);
	return 0;
}

static uint64_t tv_to_cookie(const struct timeval *tv)
{
	return (uint64_t)tv->tv_sec * 1000000 + tv->tv_usec;
}

/* A frame came out: it belongs to the surface whose picture carried its number */
static void frame_done(struct driver *d, struct context *c, int ci, int i, uint64_t cookie, bool bad)
{
	struct surface *s, *hit = NULL;
	int k;

	for (k = 0; k < N_SURFACE; k++) {
		s = &d->surfaces[k];
		if (!s->used || s->state != S_PENDING || s->ctx != ci || s->ctx_gen != c->gen)
			continue;
		if (s->cookie == cookie)
			hit = s;
		else if (s->cookie < cookie)
			s->state = S_FAILED;	/* frames come in order: this one was skipped */
	}
	if (!hit || bad) {
		if (hit)
			hit->state = S_FAILED;
		queue_cap(c, i);
		return;
	}
	hit->fd = fcntl(c->cap_fd[i], F_DUPFD_CLOEXEC, 0);
	if (hit->fd < 0) {
		hit->state = S_FAILED;
		queue_cap(c, i);
		return;
	}
	hit->cap = i;
	hit->stride = c->stride;
	hit->lines = c->lines;
	hit->size = c->size;
	hit->state = S_READY;
}

static void take_frames(struct driver *d, struct context *c, int ci)
{
	while (c->capturing) {
		struct v4l2_plane pl = { 0 };
		struct v4l2_buffer b = {
			.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, .memory = V4L2_MEMORY_MMAP,
			.length = 1, .m.planes = &pl,
		};

		if (xioctl(c->fd, VIDIOC_DQBUF, &b) < 0)
			return;
		c->queued--;
		if ((int)b.index >= c->ncap)
			continue;
		if (!pl.bytesused) {	/* the empty buffer at the end of a stream */
			queue_cap(c, b.index);
			continue;
		}
		frame_done(d, c, ci, b.index, tv_to_cookie(&b.timestamp), b.flags & V4L2_BUF_FLAG_ERROR);
	}
}

/* Stream buffers the decoder has read are free again */
static void take_stream_buffers(struct driver *d, struct context *c, int ci)
{
	for (;;) {
		struct v4l2_plane pl = { 0 };
		struct v4l2_buffer b = {
			.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE, .memory = V4L2_MEMORY_MMAP,
			.length = 1, .m.planes = &pl,
		};
		int k;

		if (xioctl(c->fd, VIDIOC_DQBUF, &b) < 0)
			return;
		if ((int)b.index < c->nout)
			c->out_busy[b.index] = false;
		if (!(b.flags & V4L2_BUF_FLAG_ERROR))
			continue;
		/* The decoder could not read it: no frame will come for it */
		for (k = 0; k < N_SURFACE; k++) {
			struct surface *s = &d->surfaces[k];

			if (s->used && s->state == S_PENDING && s->ctx == ci && s->ctx_gen == c->gen &&
			    s->cookie == tv_to_cookie(&b.timestamp))
				s->state = S_FAILED;
		}
	}
}

static void take_events(struct context *c)
{
	struct v4l2_event ev;

	while (!xioctl(c->fd, VIDIOC_DQEVENT, &ev)) {
		if (ev.type != V4L2_EVENT_SOURCE_CHANGE)
			continue;
		if (c->capturing) {
			/* The application makes a new context for a new size */
			dbg("the picture changed in the middle of the stream");
			c->failed = true;
		} else if (start_capture(c) < 0) {
			c->failed = true;
		}
	}
}

/*
 * Wait for the decoder for at most @timeout_ms and take what it has.
 * Returns 0 after a timeout, -1 when there is nothing to wait for.
 */
static int pump(struct driver *d, struct context *c, int timeout_ms, bool want_out)
{
	int ci = c - d->contexts;
	struct pollfd p = {
		.fd = c->fd,
		.events = POLLPRI | (c->capturing ? POLLIN : 0) | (want_out ? POLLOUT : 0),
	};
	int r = poll(&p, 1, timeout_ms);

	if (r < 0)
		return errno == EINTR ? 1 : -1;
	if (!r)
		return 0;
	take_events(c);
	take_frames(d, c, ci);
	take_stream_buffers(d, c, ci);
	if (!(p.revents & (POLLPRI | POLLIN | POLLOUT)))
		return -1;	/* nothing is queued on either side */
	return 1;
}

/* Queue @len bytes of the stream with the number @cookie */
static int submit(struct driver *d, struct context *c, const uint8_t *data, size_t len, uint64_t cookie)
{
	int ci = c - d->contexts;
	uint64_t end = now_ms() + SUBMIT_WAIT_MS;
	struct v4l2_plane pl = { 0 };
	struct v4l2_buffer b = {
		.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE, .memory = V4L2_MEMORY_MMAP,
		.length = 1, .m.planes = &pl,
	};
	int slot;

	if (!len || len > c->out_len) {
		dbg("a picture of %zu bytes does not fit in %zu", len, c->out_len);
		return -1;
	}
	for (;;) {
		take_stream_buffers(d, c, ci);
		for (slot = 0; slot < c->nout && c->out_busy[slot]; slot++)
			;
		if (slot < c->nout)
			break;
		if (c->failed || now_ms() >= end) {
			dbg("no free stream buffer");
			return -1;
		}
		if (pump(d, c, 20, true) < 0)
			usleep(1000);
	}
	memcpy(c->out_map[slot], data, len);
	b.index = slot;
	pl.bytesused = len;
	b.timestamp.tv_sec = cookie / 1000000;
	b.timestamp.tv_usec = cookie % 1000000;
	if (xioctl(c->fd, VIDIOC_QBUF, &b) < 0) {
		dbg("queue stream buffer: %s", strerror(errno));
		return -1;
	}
	c->out_busy[slot] = true;
	return 0;
}

static void context_close(struct driver *d, struct context *c)
{
	int ci = c - d->contexts, type, i;
	uint64_t end = now_ms() + CLOSE_WAIT_MS;
	bool pending;

	/* Pictures already sent may be shown after the context is gone */
	do {
		pending = false;
		for (i = 0; i < N_SURFACE; i++) {
			struct surface *s = &d->surfaces[i];

			if (s->used && s->state == S_PENDING && s->ctx == ci && s->ctx_gen == c->gen)
				pending = true;
		}
	} while (pending && c->capturing && !c->failed && now_ms() < end && pump(d, c, 10, false) >= 0);

	for (i = 0; i < N_SURFACE; i++) {
		struct surface *s = &d->surfaces[i];

		if (!s->used || s->ctx != ci || s->ctx_gen != c->gen)
			continue;
		if (s->state == S_PENDING)
			s->state = S_FAILED;
		s->cap = -1;	/* the frame lives on in its dma-buf */
		s->ctx = -1;
	}
	if (c->fd >= 0) {
		type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
		xioctl(c->fd, VIDIOC_STREAMOFF, &type);
		type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
		xioctl(c->fd, VIDIOC_STREAMOFF, &type);
		for (i = 0; i < c->nout; i++)
			if (c->out_map[i])
				munmap(c->out_map[i], c->out_len);
		for (i = 0; i < c->ncap; i++)
			close(c->cap_fd[i]);
		close(c->fd);
	}
	free(c->au);
	memset(c, 0, sizeof(*c));
}

static VAStatus context_open(struct driver *d, struct context *c)
{
	struct v4l2_format f = { .type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE };
	struct v4l2_requestbuffers rb = {
		.count = N_OUT, .type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE, .memory = V4L2_MEMORY_MMAP,
	};
	struct v4l2_event_subscription sub = { .type = V4L2_EVENT_SOURCE_CHANGE };
	int type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
	unsigned int i;

	c->fd = open(d->node, O_RDWR | O_NONBLOCK | O_CLOEXEC);
	if (c->fd < 0) {
		dbg("%s: %s", d->node, strerror(errno));
		return errno == EBUSY ? VA_STATUS_ERROR_HW_BUSY : VA_STATUS_ERROR_OPERATION_FAILED;
	}
	f.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_VP9;
	f.fmt.pix_mp.width = c->width;
	f.fmt.pix_mp.height = c->height;
	f.fmt.pix_mp.num_planes = 1;
	/* A key frame has to fit: the decoder keeps this between one and eight megabytes */
	f.fmt.pix_mp.plane_fmt[0].sizeimage = (uint32_t)c->width * c->height;
	if (xioctl(c->fd, VIDIOC_S_FMT, &f) < 0 || f.fmt.pix_mp.pixelformat != V4L2_PIX_FMT_VP9 ||
	    xioctl(c->fd, VIDIOC_REQBUFS, &rb) < 0 || rb.count < 2 ||
	    xioctl(c->fd, VIDIOC_SUBSCRIBE_EVENT, &sub) < 0)
		goto fail;
	c->out_len = f.fmt.pix_mp.plane_fmt[0].sizeimage;
	for (i = 0; i < rb.count && i < N_OUT; i++) {
		struct v4l2_plane pl = { 0 };
		struct v4l2_buffer b = {
			.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE, .memory = V4L2_MEMORY_MMAP,
			.index = i, .length = 1, .m.planes = &pl,
		};
		void *map;

		if (xioctl(c->fd, VIDIOC_QUERYBUF, &b) < 0 || pl.length < c->out_len)
			goto fail;
		map = mmap(NULL, c->out_len, PROT_READ | PROT_WRITE, MAP_SHARED, c->fd, pl.m.mem_offset);
		if (map == MAP_FAILED)
			goto fail;
		c->out_map[i] = map;
		c->nout = i + 1;
	}
	/* This is where the decoder takes its one session: busy if another has it */
	if (xioctl(c->fd, VIDIOC_STREAMON, &type) < 0) {
		VAStatus st = errno == EBUSY ? VA_STATUS_ERROR_HW_BUSY : VA_STATUS_ERROR_OPERATION_FAILED;

		dbg("start of the stream: %s", strerror(errno));
		context_close(d, c);
		return st;
	}
	return VA_STATUS_SUCCESS;
fail:
	dbg("set-up of the decoder: %s", strerror(errno));
	context_close(d, c);
	return VA_STATUS_ERROR_OPERATION_FAILED;
}

/* What a surface holds goes away, and its frame buffer goes back to the decoder */
static void surface_release(struct driver *d, struct surface *s)
{
	struct context *c = surface_context(d, s);

	if (s->fd >= 0)
		close(s->fd);
	s->fd = -1;
	if (s->cap >= 0 && c && c->capturing)
		queue_cap(c, s->cap);
	s->cap = -1;
	s->state = S_EMPTY;
}

static VAStatus surface_wait(struct driver *d, struct surface *s, uint64_t timeout_ms)
{
	uint64_t end = now_ms() + timeout_ms;

	while (s->state == S_PENDING) {
		struct context *c = surface_context(d, s);
		uint64_t now = now_ms();

		if (!c || c->failed) {
			s->state = S_FAILED;
			break;
		}
		if (now >= end)
			return VA_STATUS_ERROR_TIMEDOUT;
		if (pump(d, c, end - now > 50 ? 50 : (int)(end - now), false) < 0)
			usleep(1000);
	}
	return s->state == S_FAILED ? VA_STATUS_ERROR_DECODING_ERROR : VA_STATUS_SUCCESS;
}

/* The start of a VP9 frame: what the driver has to know about it */
struct vp9_head {
	bool show_existing, show, key;
	uint8_t refresh;
	int profile;
};

static int vp9_parse(const uint8_t *p, size_t len, struct vp9_head *h)
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

	memset(h, 0, sizeof(*h));
	if (BITS(2) != 2)
		return -1;
	h->profile = BITS(1);
	h->profile |= BITS(1) << 1;
	if (h->profile == 3)
		(void)BITS(1);
	h->show_existing = BITS(1);
	if (h->show_existing)
		return 0;
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
	if (intra_only) {
		if (BITS(24) != 0x498342)
			return -1;
		if (h->profile > 0)
			return -1;	/* more bits here: not a stream this driver takes */
	}
	h->refresh = BITS(8);
	return 0;
#undef BITS
}

/* VA-API: what there is */

static VAStatus va_QueryConfigProfiles(VADriverContextP ctx, VAProfile *list, int *num)
{
	(void)ctx;
	list[0] = VAProfileVP9Profile0;
	*num = 1;
	return VA_STATUS_SUCCESS;
}

static VAStatus va_QueryConfigEntrypoints(VADriverContextP ctx, VAProfile profile, VAEntrypoint *list, int *num)
{
	(void)ctx;
	if (profile != VAProfileVP9Profile0)
		return VA_STATUS_ERROR_UNSUPPORTED_PROFILE;
	list[0] = VAEntrypointVLD;
	*num = 1;
	return VA_STATUS_SUCCESS;
}

static unsigned int config_attrib(VAConfigAttribType type)
{
	switch (type) {
	case VAConfigAttribRTFormat:
		return VA_RT_FORMAT_YUV420;
	case VAConfigAttribMaxPictureWidth:
		return MAX_W;
	case VAConfigAttribMaxPictureHeight:
		return MAX_H;
	case VAConfigAttribDecSliceMode:
		return VA_DEC_SLICE_MODE_NORMAL;
	default:
		return VA_ATTRIB_NOT_SUPPORTED;
	}
}

static VAStatus va_GetConfigAttributes(VADriverContextP ctx, VAProfile profile, VAEntrypoint entrypoint,
				       VAConfigAttrib *attribs, int num)
{
	int i;

	(void)ctx;
	if (profile != VAProfileVP9Profile0)
		return VA_STATUS_ERROR_UNSUPPORTED_PROFILE;
	if (entrypoint != VAEntrypointVLD)
		return VA_STATUS_ERROR_UNSUPPORTED_ENTRYPOINT;
	for (i = 0; i < num; i++)
		attribs[i].value = config_attrib(attribs[i].type);
	return VA_STATUS_SUCCESS;
}

static VAStatus va_CreateConfig(VADriverContextP ctx, VAProfile profile, VAEntrypoint entrypoint,
				VAConfigAttrib *attribs, int num, VAConfigID *id)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_ERROR_MAX_NUM_EXCEEDED;
	int i;

	if (profile != VAProfileVP9Profile0)
		return VA_STATUS_ERROR_UNSUPPORTED_PROFILE;
	if (entrypoint != VAEntrypointVLD)
		return VA_STATUS_ERROR_UNSUPPORTED_ENTRYPOINT;
	for (i = 0; i < num; i++)
		if (attribs[i].type == VAConfigAttribRTFormat && !(attribs[i].value & VA_RT_FORMAT_YUV420))
			return VA_STATUS_ERROR_UNSUPPORTED_RT_FORMAT;
	pthread_mutex_lock(&d->lock);
	for (i = 0; i < N_CONFIG; i++) {
		if (d->configs[i].used)
			continue;
		d->configs[i].used = true;
		d->configs[i].profile = profile;
		*id = ID_CONFIG + i;
		st = VA_STATUS_SUCCESS;
		break;
	}
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_DestroyConfig(VADriverContextP ctx, VAConfigID id)
{
	struct driver *d = drv(ctx);
	struct config *c;

	pthread_mutex_lock(&d->lock);
	c = get_config(d, id);
	if (c)
		c->used = false;
	pthread_mutex_unlock(&d->lock);
	return c ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_CONFIG;
}

static VAStatus va_QueryConfigAttributes(VADriverContextP ctx, VAConfigID id, VAProfile *profile,
					 VAEntrypoint *entrypoint, VAConfigAttrib *attribs, int *num)
{
	static const VAConfigAttribType types[] = {
		VAConfigAttribRTFormat, VAConfigAttribMaxPictureWidth, VAConfigAttribMaxPictureHeight,
		VAConfigAttribDecSliceMode,
	};
	struct driver *d = drv(ctx);
	struct config *c;
	unsigned int i;

	pthread_mutex_lock(&d->lock);
	c = get_config(d, id);
	if (c)
		*profile = c->profile;
	pthread_mutex_unlock(&d->lock);
	if (!c)
		return VA_STATUS_ERROR_INVALID_CONFIG;
	*entrypoint = VAEntrypointVLD;
	for (i = 0; i < sizeof(types) / sizeof(types[0]); i++) {
		attribs[i].type = types[i];
		attribs[i].value = config_attrib(types[i]);
	}
	*num = i;
	return VA_STATUS_SUCCESS;
}

static VAStatus va_QuerySurfaceAttributes(VADriverContextP ctx, VAConfigID config, VASurfaceAttrib *attribs,
					  unsigned int *num)
{
	static const struct {
		VASurfaceAttribType type;
		unsigned int flags;
		int value;
	} list[] = {
		{ VASurfaceAttribPixelFormat, VA_SURFACE_ATTRIB_GETTABLE | VA_SURFACE_ATTRIB_SETTABLE,
		  VA_FOURCC_NV12 },
		{ VASurfaceAttribMinWidth, VA_SURFACE_ATTRIB_GETTABLE, MIN_W },
		{ VASurfaceAttribMinHeight, VA_SURFACE_ATTRIB_GETTABLE, MIN_H },
		{ VASurfaceAttribMaxWidth, VA_SURFACE_ATTRIB_GETTABLE, MAX_W },
		{ VASurfaceAttribMaxHeight, VA_SURFACE_ATTRIB_GETTABLE, MAX_H },
		{ VASurfaceAttribMemoryType, VA_SURFACE_ATTRIB_GETTABLE | VA_SURFACE_ATTRIB_SETTABLE,
		  VA_SURFACE_ATTRIB_MEM_TYPE_VA },
	};
	const unsigned int n = sizeof(list) / sizeof(list[0]);
	unsigned int i;

	(void)ctx;
	(void)config;
	if (!attribs) {
		*num = n;
		return VA_STATUS_SUCCESS;
	}
	if (*num < n) {
		*num = n;
		return VA_STATUS_ERROR_MAX_NUM_EXCEEDED;
	}
	for (i = 0; i < n; i++) {
		attribs[i].type = list[i].type;
		attribs[i].flags = list[i].flags;
		attribs[i].value.type = VAGenericValueTypeInteger;
		attribs[i].value.value.i = list[i].value;
	}
	*num = n;
	return VA_STATUS_SUCCESS;
}

/* VA-API: surfaces */

static VAStatus va_CreateSurfaces2(VADriverContextP ctx, unsigned int format, unsigned int width,
				   unsigned int height, VASurfaceID *ids, unsigned int num,
				   VASurfaceAttrib *attribs, unsigned int num_attribs)
{
	struct driver *d = drv(ctx);
	unsigned int i, k, made = 0;

	if (format != VA_RT_FORMAT_YUV420)
		return VA_STATUS_ERROR_UNSUPPORTED_RT_FORMAT;
	if (!width || !height || width > MAX_W || height > MAX_H)
		return VA_STATUS_ERROR_RESOLUTION_NOT_SUPPORTED;
	for (i = 0; i < num_attribs; i++) {
		if (!(attribs[i].flags & VA_SURFACE_ATTRIB_SETTABLE))
			continue;
		if (attribs[i].type == VASurfaceAttribPixelFormat &&
		    attribs[i].value.value.i != (int)VA_FOURCC_NV12)
			return VA_STATUS_ERROR_UNSUPPORTED_RT_FORMAT;
		/* Frames are the decoder's own buffers: nothing is brought in from outside */
		if (attribs[i].type == VASurfaceAttribMemoryType &&
		    attribs[i].value.value.i != VA_SURFACE_ATTRIB_MEM_TYPE_VA)
			return VA_STATUS_ERROR_UNSUPPORTED_MEMORY_TYPE;
		if (attribs[i].type == VASurfaceAttribExternalBufferDescriptor)
			return VA_STATUS_ERROR_UNSUPPORTED_MEMORY_TYPE;
	}
	pthread_mutex_lock(&d->lock);
	for (k = 0; k < N_SURFACE && made < num; k++) {
		struct surface *s = &d->surfaces[k];

		if (s->used)
			continue;
		memset(s, 0, sizeof(*s));
		s->used = true;
		s->width = width;
		s->height = height;
		s->ctx = -1;
		s->cap = -1;
		s->fd = -1;
		ids[made++] = ID_SURFACE + k;
	}
	if (made < num) {
		for (i = 0; i < made; i++)
			d->surfaces[ids[i] - ID_SURFACE].used = false;
		pthread_mutex_unlock(&d->lock);
		return VA_STATUS_ERROR_MAX_NUM_EXCEEDED;
	}
	pthread_mutex_unlock(&d->lock);
	return VA_STATUS_SUCCESS;
}

static VAStatus va_CreateSurfaces(VADriverContextP ctx, int width, int height, int format, int num,
				  VASurfaceID *ids)
{
	return va_CreateSurfaces2(ctx, format, width, height, ids, num, NULL, 0);
}

static VAStatus va_DestroySurfaces(VADriverContextP ctx, VASurfaceID *ids, int num)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_SUCCESS;
	int i;

	pthread_mutex_lock(&d->lock);
	for (i = 0; i < num; i++) {
		struct surface *s = get_surface(d, ids[i]);

		if (!s) {
			st = VA_STATUS_ERROR_INVALID_SURFACE;
			continue;
		}
		surface_release(d, s);
		s->used = false;
	}
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_SyncSurface2(VADriverContextP ctx, VASurfaceID id, uint64_t timeout_ns)
{
	struct driver *d = drv(ctx);
	struct surface *s;
	VAStatus st = VA_STATUS_ERROR_INVALID_SURFACE;

	pthread_mutex_lock(&d->lock);
	s = get_surface(d, id);
	if (s)
		st = surface_wait(d, s, timeout_ns == VA_TIMEOUT_INFINITE ? EXPORT_WAIT_MS :
				  timeout_ns / 1000000);
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_SyncSurface(VADriverContextP ctx, VASurfaceID id)
{
	return va_SyncSurface2(ctx, id, VA_TIMEOUT_INFINITE);
}

static VAStatus va_QuerySurfaceStatus(VADriverContextP ctx, VASurfaceID id, VASurfaceStatus *status)
{
	struct driver *d = drv(ctx);
	struct surface *s;

	pthread_mutex_lock(&d->lock);
	s = get_surface(d, id);
	if (s) {
		surface_wait(d, s, 0);
		*status = s->state == S_PENDING ? VASurfaceRendering : VASurfaceReady;
	}
	pthread_mutex_unlock(&d->lock);
	return s ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_SURFACE;
}

static VAStatus va_ExportSurfaceHandle(VADriverContextP ctx, VASurfaceID id, uint32_t mem_type, uint32_t flags,
				       void *descriptor)
{
	struct driver *d = drv(ctx);
	VADRMPRIMESurfaceDescriptor *p = descriptor;
	struct surface *s;
	VAStatus st;
	int fd;

	if (mem_type != VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2)
		return VA_STATUS_ERROR_UNSUPPORTED_MEMORY_TYPE;
	pthread_mutex_lock(&d->lock);
	s = get_surface(d, id);
	if (!s) {
		pthread_mutex_unlock(&d->lock);
		return VA_STATUS_ERROR_INVALID_SURFACE;
	}
	/* The application asks for the frame before it waits for it */
	st = surface_wait(d, s, EXPORT_WAIT_MS);
	if (st == VA_STATUS_SUCCESS && s->state != S_READY)
		st = VA_STATUS_ERROR_INVALID_SURFACE;	/* nothing was decoded into it */
	if (st != VA_STATUS_SUCCESS) {
		dbg("export of surface %#x: state %d, status %#x", id, s->state, st);
		pthread_mutex_unlock(&d->lock);
		return st;
	}
	fd = fcntl(s->fd, F_DUPFD_CLOEXEC, 0);
	if (fd < 0) {
		pthread_mutex_unlock(&d->lock);
		return VA_STATUS_ERROR_ALLOCATION_FAILED;
	}
	memset(p, 0, sizeof(*p));
	p->fourcc = VA_FOURCC_NV12;
	p->width = s->width;
	p->height = s->height;
	p->num_objects = 1;
	p->objects[0].fd = fd;
	p->objects[0].size = s->size;
	p->objects[0].drm_format_modifier = DRM_FORMAT_MOD_LINEAR;
	if (flags & VA_EXPORT_SURFACE_COMPOSED_LAYERS) {
		p->num_layers = 1;
		p->layers[0].drm_format = DRM_FORMAT_NV12;
		p->layers[0].num_planes = 2;
		p->layers[0].pitch[0] = s->stride;
		p->layers[0].offset[1] = s->stride * s->lines;
		p->layers[0].pitch[1] = s->stride;
	} else {
		p->num_layers = 2;
		p->layers[0].drm_format = DRM_FORMAT_R8;
		p->layers[0].num_planes = 1;
		p->layers[0].pitch[0] = s->stride;
		p->layers[1].drm_format = DRM_FORMAT_GR88;
		p->layers[1].num_planes = 1;
		p->layers[1].offset[0] = s->stride * s->lines;
		p->layers[1].pitch[0] = s->stride;
	}
	pthread_mutex_unlock(&d->lock);
	return VA_STATUS_SUCCESS;
}

/* VA-API: contexts and pictures */

static VAStatus va_CreateContext(VADriverContextP ctx, VAConfigID config, int width, int height, int flag,
				 VASurfaceID *targets, int num_targets, VAContextID *id)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_ERROR_MAX_NUM_EXCEEDED;
	int i;

	(void)flag;
	(void)targets;
	(void)num_targets;
	if (width < MIN_W || height < MIN_H || width > MAX_W || height > MAX_H)
		return VA_STATUS_ERROR_RESOLUTION_NOT_SUPPORTED;
	pthread_mutex_lock(&d->lock);
	if (!get_config(d, config)) {
		pthread_mutex_unlock(&d->lock);
		return VA_STATUS_ERROR_INVALID_CONFIG;
	}
	for (i = 0; i < N_CONTEXT; i++) {
		struct context *c = &d->contexts[i];

		if (c->used)
			continue;
		memset(c, 0, sizeof(*c));
		c->width = width;
		c->height = height;
		c->target = -1;
		st = context_open(d, c);
		if (st != VA_STATUS_SUCCESS)
			break;
		c->used = true;
		c->gen = ++d->gen;
		*id = ID_CONTEXT + i;
		dbg("context %d: %dx%d", i, width, height);
		break;
	}
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_DestroyContext(VADriverContextP ctx, VAContextID id)
{
	struct driver *d = drv(ctx);
	struct context *c;

	pthread_mutex_lock(&d->lock);
	c = get_context(d, id);
	if (c)
		context_close(d, c);
	pthread_mutex_unlock(&d->lock);
	return c ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_CONTEXT;
}

static VAStatus va_CreateBuffer(VADriverContextP ctx, VAContextID context, VABufferType type, unsigned int size,
				unsigned int count, void *data, VABufferID *id)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_ERROR_MAX_NUM_EXCEEDED;
	void *mem;
	int i;

	(void)context;
	if (!size || !count || (uint64_t)size * count > (64u << 20))
		return VA_STATUS_ERROR_INVALID_PARAMETER;
	mem = malloc((size_t)size * count);
	if (!mem)
		return VA_STATUS_ERROR_ALLOCATION_FAILED;
	if (data)
		memcpy(mem, data, (size_t)size * count);
	else
		memset(mem, 0, (size_t)size * count);
	pthread_mutex_lock(&d->lock);
	for (i = 0; i < N_BUFFER; i++) {
		struct buffer *b = &d->buffers[i];

		if (b->used)
			continue;
		b->used = true;
		b->type = type;
		b->size = size;
		b->count = count;
		b->data = mem;
		*id = ID_BUFFER + i;
		st = VA_STATUS_SUCCESS;
		break;
	}
	pthread_mutex_unlock(&d->lock);
	if (st != VA_STATUS_SUCCESS)
		free(mem);
	return st;
}

static VAStatus va_BufferSetNumElements(VADriverContextP ctx, VABufferID id, unsigned int count)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_ERROR_INVALID_BUFFER;
	struct buffer *b;

	pthread_mutex_lock(&d->lock);
	b = get_buffer(d, id);
	if (b && count && count <= b->count) {
		b->count = count;
		st = VA_STATUS_SUCCESS;
	} else if (b) {
		st = VA_STATUS_ERROR_INVALID_PARAMETER;
	}
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_MapBuffer(VADriverContextP ctx, VABufferID id, void **data)
{
	struct driver *d = drv(ctx);
	struct buffer *b;

	pthread_mutex_lock(&d->lock);
	b = get_buffer(d, id);
	if (b)
		*data = b->data;
	pthread_mutex_unlock(&d->lock);
	return b ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_BUFFER;
}

#if VA_CHECK_VERSION(1, 21, 0)
static VAStatus va_MapBuffer2(VADriverContextP ctx, VABufferID id, void **data, uint32_t flags)
{
	(void)flags;
	return va_MapBuffer(ctx, id, data);
}
#endif

static VAStatus va_UnmapBuffer(VADriverContextP ctx, VABufferID id)
{
	struct driver *d = drv(ctx);
	struct buffer *b;

	pthread_mutex_lock(&d->lock);
	b = get_buffer(d, id);
	pthread_mutex_unlock(&d->lock);
	return b ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_BUFFER;
}

static VAStatus va_DestroyBuffer(VADriverContextP ctx, VABufferID id)
{
	struct driver *d = drv(ctx);
	struct buffer *b;

	pthread_mutex_lock(&d->lock);
	b = get_buffer(d, id);
	if (b) {
		free(b->data);
		memset(b, 0, sizeof(*b));
	}
	pthread_mutex_unlock(&d->lock);
	return b ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_BUFFER;
}

static VAStatus va_BufferInfo(VADriverContextP ctx, VABufferID id, VABufferType *type, unsigned int *size,
			      unsigned int *count)
{
	struct driver *d = drv(ctx);
	struct buffer *b;

	pthread_mutex_lock(&d->lock);
	b = get_buffer(d, id);
	if (b) {
		*type = b->type;
		*size = b->size;
		*count = b->count;
	}
	pthread_mutex_unlock(&d->lock);
	return b ? VA_STATUS_SUCCESS : VA_STATUS_ERROR_INVALID_BUFFER;
}

static VAStatus va_BeginPicture(VADriverContextP ctx, VAContextID context, VASurfaceID target)
{
	struct driver *d = drv(ctx);
	struct context *c;
	struct surface *s;
	VAStatus st = VA_STATUS_SUCCESS;

	pthread_mutex_lock(&d->lock);
	c = get_context(d, context);
	s = get_surface(d, target);
	if (!c) {
		st = VA_STATUS_ERROR_INVALID_CONTEXT;
	} else if (!s) {
		st = VA_STATUS_ERROR_INVALID_SURFACE;
	} else {
		/* Whatever the surface held is no longer wanted */
		surface_release(d, s);
		s->ctx = c - d->contexts;
		s->ctx_gen = c->gen;
		c->target = s - d->surfaces;
		c->au_len = 0;
	}
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_RenderPicture(VADriverContextP ctx, VAContextID context, VABufferID *ids, int num)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_SUCCESS;
	struct context *c;
	int i;

	pthread_mutex_lock(&d->lock);
	c = get_context(d, context);
	if (!c || c->target < 0)
		st = VA_STATUS_ERROR_INVALID_CONTEXT;
	for (i = 0; st == VA_STATUS_SUCCESS && i < num; i++) {
		struct buffer *b = get_buffer(d, ids[i]);
		size_t len;

		if (!b) {
			st = VA_STATUS_ERROR_INVALID_BUFFER;
			break;
		}
		/* The decoder reads the stream itself: only the bytes of the picture matter */
		if (b->type != VASliceDataBufferType)
			continue;
		len = (size_t)b->size * b->count;
		if (c->au_len + len > c->au_cap) {
			size_t cap = (c->au_len + len) * 2;
			uint8_t *au = realloc(c->au, cap);

			if (!au) {
				st = VA_STATUS_ERROR_ALLOCATION_FAILED;
				break;
			}
			c->au = au;
			c->au_cap = cap;
		}
		memcpy(c->au + c->au_len, b->data, len);
		c->au_len += len;
	}
	pthread_mutex_unlock(&d->lock);
	return st;
}

static VAStatus va_EndPicture(VADriverContextP ctx, VAContextID context)
{
	struct driver *d = drv(ctx);
	VAStatus st = VA_STATUS_ERROR_DECODING_ERROR;
	struct context *c;
	struct surface *s;
	struct vp9_head h;
	uint64_t cookie, end;
	int ci, k, pending;

	pthread_mutex_lock(&d->lock);
	c = get_context(d, context);
	if (!c || c->target < 0) {
		pthread_mutex_unlock(&d->lock);
		return VA_STATUS_ERROR_INVALID_CONTEXT;
	}
	ci = c - d->contexts;
	s = &d->surfaces[c->target];
	c->target = -1;
	if (c->failed)
		goto out;
	if (vp9_parse(c->au, c->au_len, &h) < 0 || h.show_existing || h.profile != 0) {
		dbg("a picture of %zu bytes that is not a VP9 profile 0 frame", c->au_len);
		goto out;
	}
	if (!c->capturing && !h.key) {
		/* The decoder finds its headers in a key frame; what comes before one is lost anyway */
		dbg("a picture before the first key frame is dropped");
		st = VA_STATUS_SUCCESS;
		goto out;
	}

	cookie = ++c->seq;
	if (h.show || h.refresh) {
		s->state = S_PENDING;
		s->cookie = cookie;
		/* Every picture that waits for its frame needs a buffer the decoder can fill */
		for (pending = 0, k = 0; k < N_SURFACE; k++) {
			struct surface *o = &d->surfaces[k];

			pending += o->used && o->state == S_PENDING && o->ctx == ci && o->ctx_gen == c->gen;
		}
		while (c->capturing && c->queued < pending && !add_cap(c))
			;
	}
	if (submit(d, c, c->au, c->au_len, cookie) < 0) {
		s->state = S_FAILED;
		goto out;
	}
	if (!h.show && h.refresh) {
		/* Decoded and not shown: have the decoder put it out all the same */
		uint8_t show = 0x88 | __builtin_ctz(h.refresh);

		if (submit(d, c, &show, 1, cookie) < 0) {
			s->state = S_FAILED;
			goto out;
		}
	}
	/* The first picture: wait until the decoder has read the headers and frames can flow */
	end = now_ms() + HEADER_WAIT_MS;
	while (!c->capturing && !c->failed) {
		bool busy = false;

		if (now_ms() >= end) {
			dbg("the decoder did not find the headers in time");
			c->failed = true;
			break;
		}
		if (pump(d, c, 50, true) < 0)
			usleep(1000);
		for (k = 0; k < c->nout; k++)
			busy |= c->out_busy[k];
		if (!c->capturing && !busy) {
			dbg("no headers in the first picture");
			s->state = S_FAILED;
			goto out;
		}
	}
	if (c->failed) {
		s->state = S_FAILED;
		goto out;
	}
	take_frames(d, c, ci);
	st = VA_STATUS_SUCCESS;
out:
	pthread_mutex_unlock(&d->lock);
	return st;
}

/* VA-API: reading a frame with the CPU (slow: the buffers are not cached memory) */

static VAStatus va_QueryImageFormats(VADriverContextP ctx, VAImageFormat *list, int *num)
{
	(void)ctx;
	memset(list, 0, sizeof(*list));
	list[0].fourcc = VA_FOURCC_NV12;
	list[0].byte_order = VA_LSB_FIRST;
	list[0].bits_per_pixel = 12;
	*num = 1;
	return VA_STATUS_SUCCESS;
}

static VAStatus va_CreateImage(VADriverContextP ctx, VAImageFormat *format, int width, int height, VAImage *out)
{
	struct driver *d = drv(ctx);
	VAStatus st;
	VAImage va;
	int i, w = (width + 1) & ~1, h = (height + 1) & ~1;

	if (format->fourcc != VA_FOURCC_NV12)
		return VA_STATUS_ERROR_INVALID_IMAGE_FORMAT;
	if (width <= 0 || height <= 0 || width > MAX_W || height > MAX_H)
		return VA_STATUS_ERROR_INVALID_PARAMETER;
	memset(&va, 0, sizeof(va));
	va.format = *format;
	va.width = width;
	va.height = height;
	va.data_size = w * h * 3 / 2;
	va.num_planes = 2;
	va.pitches[0] = w;
	va.pitches[1] = w;
	va.offsets[1] = w * h;
	st = va_CreateBuffer(ctx, VA_INVALID_ID, VAImageBufferType, va.data_size, 1, NULL, &va.buf);
	if (st != VA_STATUS_SUCCESS)
		return st;
	st = VA_STATUS_ERROR_MAX_NUM_EXCEEDED;
	pthread_mutex_lock(&d->lock);
	for (i = 0; i < N_IMAGE; i++) {
		if (d->images[i].used)
			continue;
		va.image_id = ID_IMAGE + i;
		d->images[i].used = true;
		d->images[i].va = va;
		*out = va;
		st = VA_STATUS_SUCCESS;
		break;
	}
	pthread_mutex_unlock(&d->lock);
	if (st != VA_STATUS_SUCCESS)
		va_DestroyBuffer(ctx, va.buf);
	return st;
}

static VAStatus va_DestroyImage(VADriverContextP ctx, VAImageID id)
{
	struct driver *d = drv(ctx);
	VABufferID buf = VA_INVALID_ID;
	struct image *im;

	pthread_mutex_lock(&d->lock);
	im = get_image(d, id);
	if (im) {
		buf = im->va.buf;
		im->used = false;
	}
	pthread_mutex_unlock(&d->lock);
	if (!im)
		return VA_STATUS_ERROR_INVALID_IMAGE;
	va_DestroyBuffer(ctx, buf);
	return VA_STATUS_SUCCESS;
}

static VAStatus va_GetImage(VADriverContextP ctx, VASurfaceID surface, int x, int y, unsigned int width,
			    unsigned int height, VAImageID image)
{
	struct driver *d = drv(ctx);
	struct surface *s;
	struct image *im;
	struct buffer *b;
	const uint8_t *src;
	uint8_t *dst;
	unsigned int row;
	VAStatus st;

	pthread_mutex_lock(&d->lock);
	s = get_surface(d, surface);
	im = get_image(d, image);
	b = im ? get_buffer(d, im->va.buf) : NULL;
	if (!s || !im || !b) {
		pthread_mutex_unlock(&d->lock);
		return s ? VA_STATUS_ERROR_INVALID_IMAGE : VA_STATUS_ERROR_INVALID_SURFACE;
	}
	st = surface_wait(d, s, EXPORT_WAIT_MS);
	if (st == VA_STATUS_SUCCESS && s->state != S_READY)
		st = VA_STATUS_ERROR_INVALID_SURFACE;
	if (st == VA_STATUS_SUCCESS &&
	    (x < 0 || y < 0 || (x & 1) || (y & 1) || x + width > (unsigned int)s->width ||
	     y + height > (unsigned int)s->height || width > im->va.width || height > im->va.height))
		st = VA_STATUS_ERROR_INVALID_PARAMETER;
	if (st != VA_STATUS_SUCCESS) {
		pthread_mutex_unlock(&d->lock);
		return st;
	}
	src = mmap(NULL, s->size, PROT_READ, MAP_SHARED, s->fd, 0);
	if (src == MAP_FAILED) {
		pthread_mutex_unlock(&d->lock);
		return VA_STATUS_ERROR_OPERATION_FAILED;
	}
	dst = b->data;
	for (row = 0; row < height; row++)
		memcpy(dst + im->va.offsets[0] + (size_t)row * im->va.pitches[0],
		       src + (size_t)(y + row) * s->stride + x, width);
	for (row = 0; row < (height + 1) / 2; row++)
		memcpy(dst + im->va.offsets[1] + (size_t)row * im->va.pitches[1],
		       src + (size_t)s->stride * s->lines + (size_t)(y / 2 + row) * s->stride + x,
		       (width + 1) & ~1u);
	munmap((void *)src, s->size);
	pthread_mutex_unlock(&d->lock);
	return VA_STATUS_SUCCESS;
}

/* VA-API: the rest */

static VAStatus va_unsupported(VADriverContextP ctx)
{
	(void)ctx;
	return VA_STATUS_ERROR_UNIMPLEMENTED;
}

static VAStatus va_QueryDisplayAttributes(VADriverContextP ctx, VADisplayAttribute *list, int *num)
{
	(void)ctx;
	(void)list;
	*num = 0;
	return VA_STATUS_SUCCESS;
}

static VAStatus va_Terminate(VADriverContextP ctx)
{
	struct driver *d = drv(ctx);
	int i;

	pthread_mutex_lock(&d->lock);
	for (i = 0; i < N_CONTEXT; i++)
		if (d->contexts[i].used)
			context_close(d, &d->contexts[i]);
	for (i = 0; i < N_SURFACE; i++)
		if (d->surfaces[i].used && d->surfaces[i].fd >= 0)
			close(d->surfaces[i].fd);
	for (i = 0; i < N_BUFFER; i++)
		free(d->buffers[i].data);
	pthread_mutex_unlock(&d->lock);
	pthread_mutex_destroy(&d->lock);
	free(d);
	ctx->pDriverData = NULL;
	return VA_STATUS_SUCCESS;
}

static VAStatus driver_init(VADriverContextP ctx)
{
	struct VADriverVTable *vt = ctx->vtable;
	struct driver *d = calloc(1, sizeof(*d));

	if (!d)
		return VA_STATUS_ERROR_ALLOCATION_FAILED;
	if (find_node(d->node, sizeof(d->node)) < 0) {
		dbg("no " DECODER_NAME " among the video devices");
		free(d);
		return VA_STATUS_ERROR_UNKNOWN;
	}
	pthread_mutex_init(&d->lock, NULL);

	vt->vaTerminate = va_Terminate;
	vt->vaQueryConfigProfiles = va_QueryConfigProfiles;
	vt->vaQueryConfigEntrypoints = va_QueryConfigEntrypoints;
	vt->vaGetConfigAttributes = va_GetConfigAttributes;
	vt->vaCreateConfig = va_CreateConfig;
	vt->vaDestroyConfig = va_DestroyConfig;
	vt->vaQueryConfigAttributes = va_QueryConfigAttributes;
	vt->vaCreateSurfaces = va_CreateSurfaces;
	vt->vaDestroySurfaces = va_DestroySurfaces;
	vt->vaCreateContext = va_CreateContext;
	vt->vaDestroyContext = va_DestroyContext;
	vt->vaCreateBuffer = va_CreateBuffer;
	vt->vaBufferSetNumElements = va_BufferSetNumElements;
	vt->vaMapBuffer = va_MapBuffer;
	vt->vaUnmapBuffer = va_UnmapBuffer;
	vt->vaDestroyBuffer = va_DestroyBuffer;
	vt->vaBeginPicture = va_BeginPicture;
	vt->vaRenderPicture = va_RenderPicture;
	vt->vaEndPicture = va_EndPicture;
	vt->vaSyncSurface = va_SyncSurface;
	vt->vaQuerySurfaceStatus = va_QuerySurfaceStatus;
	vt->vaQuerySurfaceError = (void *)va_unsupported;
	vt->vaPutSurface = (void *)va_unsupported;
	vt->vaQueryImageFormats = va_QueryImageFormats;
	vt->vaCreateImage = va_CreateImage;
	vt->vaDeriveImage = (void *)va_unsupported;
	vt->vaDestroyImage = va_DestroyImage;
	vt->vaSetImagePalette = (void *)va_unsupported;
	vt->vaGetImage = va_GetImage;
	vt->vaPutImage = (void *)va_unsupported;
	vt->vaQuerySubpictureFormats = (void *)va_unsupported;
	vt->vaCreateSubpicture = (void *)va_unsupported;
	vt->vaDestroySubpicture = (void *)va_unsupported;
	vt->vaSetSubpictureImage = (void *)va_unsupported;
	vt->vaSetSubpictureChromakey = (void *)va_unsupported;
	vt->vaSetSubpictureGlobalAlpha = (void *)va_unsupported;
	vt->vaAssociateSubpicture = (void *)va_unsupported;
	vt->vaDeassociateSubpicture = (void *)va_unsupported;
	vt->vaQueryDisplayAttributes = va_QueryDisplayAttributes;
	vt->vaGetDisplayAttributes = (void *)va_unsupported;
	vt->vaSetDisplayAttributes = (void *)va_unsupported;
	vt->vaBufferInfo = va_BufferInfo;
	vt->vaLockSurface = (void *)va_unsupported;
	vt->vaUnlockSurface = (void *)va_unsupported;
	vt->vaGetSurfaceAttributes = (void *)va_unsupported;
	vt->vaCreateSurfaces2 = va_CreateSurfaces2;
	vt->vaQuerySurfaceAttributes = va_QuerySurfaceAttributes;
	vt->vaAcquireBufferHandle = (void *)va_unsupported;
	vt->vaReleaseBufferHandle = (void *)va_unsupported;
	vt->vaExportSurfaceHandle = va_ExportSurfaceHandle;
	vt->vaSyncSurface2 = va_SyncSurface2;
#if VA_CHECK_VERSION(1, 21, 0)
	vt->vaMapBuffer2 = va_MapBuffer2;
#endif

	ctx->version_major = VA_MAJOR_VERSION;
	ctx->version_minor = VA_MINOR_VERSION;
	ctx->max_profiles = 1;
	ctx->max_entrypoints = 1;
	ctx->max_attributes = 8;
	ctx->max_image_formats = 1;
	ctx->max_subpic_formats = 1;
	ctx->max_display_attributes = 1;
	ctx->str_vendor = "buntOS MT6897 video decoder (V4L2)";
	ctx->pDriverData = d;
	dbg("decoder at %s", d->node);
	return VA_STATUS_SUCCESS;
}

/* libva looks for the name of its own version first and for older ones after it */
__attribute__((visibility("default"))) VAStatus __vaDriverInit_1_0(VADriverContextP ctx);
VAStatus __vaDriverInit_1_0(VADriverContextP ctx)
{
	return driver_init(ctx);
}

#if defined(VA_DRIVER_INIT_FUNC) && VA_MINOR_VERSION > 0
__attribute__((visibility("default"))) VAStatus VA_DRIVER_INIT_FUNC(VADriverContextP ctx);
VAStatus VA_DRIVER_INIT_FUNC(VADriverContextP ctx)
{
	return driver_init(ctx);
}
#endif
