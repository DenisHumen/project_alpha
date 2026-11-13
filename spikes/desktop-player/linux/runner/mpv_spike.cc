// SPDX-License-Identifier: MPL-2.0
// PLAY-DSK-001 spike: two ways to put libmpv video into a Flutter Linux app.
//
//   mode "texture": libmpv render API (OpenGL, vo=libmpv) renders into an FBO
//                   on a dedicated render thread, in our own EGL context
//                   created *shared with Flutter's EGL context* (captured in
//                   the first FlTextureGL::populate() call). The texture is
//                   composited by Flutter like any widget: the Flutter OSD on
//                   top is free, but output is limited to Flutter's 8-bit SDR
//                   surface.
//   mode "native":  mpv owns a native surface and its own VO (vo=gpu-next):
//                   X11 -> native child window passed via --wid;
//                   Wayland -> mpv's own xdg_toplevel (libmpv has no --wid on
//                   Wayland). Flutter cannot draw over that surface.
//
// Lessons baked in (see ADR-0009):
//   * Flutter's Linux embedder (3.50, Impeller GLES) uses its own EGL
//     contexts; a GdkGLContext does not share textures with them.
//   * On X11 GTK keeps a GLX context current on the main thread, and libglvnd
//     refuses eglMakeCurrent there (EGL_BAD_ACCESS) -> render on own thread.
//   * mpv picks a Wayland gpu-context whenever WAYLAND_DISPLAY is set, even
//     with --wid; force an X11 context for the X11 child-window path.
//   * A GDK-managed native child window is overdrawn by GTK's toplevel
//     repaint; a plain Xlib child (unknown to GDK) is shown correctly.
//
// This is throw-away spike code: no error recovery, single player instance.

#include "mpv_spike.h"

#include <epoxy/egl.h>
#include <epoxy/gl.h>
#include <mpv/client.h>
#include <mpv/render_gl.h>
#include <sys/resource.h>

#include <atomic>
#include <chrono>
#include <clocale>
#include <condition_variable>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>

#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif
#ifdef GDK_WINDOWING_WAYLAND
#include <gdk/gdkwayland.h>
#endif

namespace {

using Clock = std::chrono::steady_clock;

struct Spike {
  FlView* view = nullptr;
  GtkWindow* window = nullptr;
  FlTextureRegistrar* registrar = nullptr;
  FlMethodChannel* channel = nullptr;

  std::string mode;
  std::string path;
  mpv_handle* mpv = nullptr;

  // texture mode
  mpv_render_context* rctx = nullptr;
  EGLDisplay egl_dpy = EGL_NO_DISPLAY;
  EGLContext egl_ctx = EGL_NO_CONTEXT;
  EGLSurface egl_surface = EGL_NO_SURFACE;  // 1x1 pbuffer (or surfaceless)
  EGLenum egl_api = EGL_OPENGL_ES_API;
  void* native_display = nullptr;  // wl_display* or Display* for hwdec interop
  bool native_display_is_wayland = false;
  GLuint fbo = 0;
  GLuint tex = 0;
  int tex_w = 0;
  int tex_h = 0;
  std::mutex tex_mutex;  // guards tex/tex_w/tex_h against populate()
  FlTexture* texture = nullptr;

  std::thread render_thread;
  std::mutex cv_mutex;
  std::condition_variable cv;
  bool ctx_ready = false;      // shared EGL context exists
  bool frame_pending = false;  // mpv asked for a redraw

  std::atomic<uint64_t> populate_calls{0};
  std::atomic<uint64_t> frames_rendered{0};
  double render_ms_total = 0;
  double render_ms_max = 0;

  // native mode (X11)
  GdkWindow* child = nullptr;

  Clock::time_point opened_at;
};

Spike* g_spike = nullptr;

bool is_wayland() {
#ifdef GDK_WINDOWING_WAYLAND
  return GDK_IS_WAYLAND_DISPLAY(gdk_display_get_default());
#else
  return false;
#endif
}

const char* env_or(const char* name, const char* fallback) {
  const char* v = g_getenv(name);
  return (v != nullptr && *v != '\0') ? v : fallback;
}

void* get_proc_address(void* ctx, const char* name) {
  return reinterpret_cast<void*>(eglGetProcAddress(name));
}

// Called inside populate(): Flutter's EGL context is current on this thread.
void create_shared_context(Spike* s) {
  EGLDisplay dpy = eglGetCurrentDisplay();
  EGLContext flutter_ctx = eglGetCurrentContext();
  if (flutter_ctx == EGL_NO_CONTEXT) {
    g_printerr("[spike] populate(): no current EGL context\n");
    return;
  }
  EGLint cfg_id = 0, client_type = 0, client_version = 0;
  eglQueryContext(dpy, flutter_ctx, EGL_CONFIG_ID, &cfg_id);
  eglQueryContext(dpy, flutter_ctx, EGL_CONTEXT_CLIENT_TYPE, &client_type);
  eglQueryContext(dpy, flutter_ctx, EGL_CONTEXT_CLIENT_VERSION, &client_version);
  EGLConfig cfg = nullptr;  // == EGL_NO_CONFIG_KHR
  if (cfg_id != 0) {
    EGLint attrs[] = {EGL_CONFIG_ID, cfg_id, EGL_NONE};
    EGLint n = 0;
    eglChooseConfig(dpy, attrs, &cfg, 1, &n);
  }
  s->egl_api = static_cast<EGLenum>(client_type);
  EGLenum prev_api = eglQueryAPI();
  eglBindAPI(s->egl_api);
  EGLint es_attrs[] = {EGL_CONTEXT_CLIENT_VERSION, client_version, EGL_NONE};
  EGLint gl_attrs[] = {EGL_NONE};
  s->egl_ctx = eglCreateContext(
      dpy, cfg, flutter_ctx,
      s->egl_api == EGL_OPENGL_ES_API ? es_attrs : gl_attrs);
  if (cfg != nullptr) {
    EGLint pb_attrs[] = {EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE};
    s->egl_surface = eglCreatePbufferSurface(dpy, cfg, pb_attrs);
  }
  eglBindAPI(prev_api);
  s->egl_dpy = dpy;
  g_printerr(
      "[spike] Flutter EGL ctx: api=%s version=%d renderer=%s config=%d; "
      "populate() on %s thread; shared ctx %s\n",
      s->egl_api == EGL_OPENGL_ES_API ? "GLES" : "GL", client_version,
      reinterpret_cast<const char*>(glGetString(GL_RENDERER)), cfg_id,
      g_main_context_is_owner(g_main_context_default()) ? "GTK main"
                                                        : "a non-main",
      s->egl_ctx != EGL_NO_CONTEXT ? "created" : "FAILED");
}

// ---------------------------------------------------------------------------
// FlTextureGL subclass: hands Flutter the texture name mpv renders into.

G_DECLARE_FINAL_TYPE(MpvGlTexture, mpv_gl_texture, MPV, GL_TEXTURE, FlTextureGL)

struct _MpvGlTexture {
  FlTextureGL parent_instance;
};

G_DEFINE_TYPE(MpvGlTexture, mpv_gl_texture, fl_texture_gl_get_type())

gboolean mpv_gl_texture_populate(FlTextureGL* texture, uint32_t* target,
                                 uint32_t* name, uint32_t* width,
                                 uint32_t* height, GError** error) {
  Spike* s = g_spike;
  s->populate_calls++;
  if (s->egl_ctx == EGL_NO_CONTEXT) {
    create_shared_context(s);
    std::lock_guard<std::mutex> lock(s->cv_mutex);
    s->ctx_ready = true;
    s->cv.notify_all();
  }
  std::lock_guard<std::mutex> lock(s->tex_mutex);
  if (s->tex == 0) {
    g_set_error(error, g_quark_from_static_string("mpv"), 1, "no frame yet");
    return FALSE;
  }
  *target = GL_TEXTURE_2D;
  *name = s->tex;
  *width = s->tex_w;
  *height = s->tex_h;
  return TRUE;
}

void mpv_gl_texture_class_init(MpvGlTextureClass* klass) {
  FL_TEXTURE_GL_CLASS(klass)->populate = mpv_gl_texture_populate;
}

void mpv_gl_texture_init(MpvGlTexture* self) {}

// ---------------------------------------------------------------------------
// Render thread.

gboolean mark_frame_available(gpointer data) {
  Spike* s = static_cast<Spike*>(data);
  fl_texture_registrar_mark_texture_frame_available(s->registrar, s->texture);
  return G_SOURCE_REMOVE;
}

void on_mpv_render_update(void* ctx) {
  Spike* s = static_cast<Spike*>(ctx);
  std::lock_guard<std::mutex> lock(s->cv_mutex);
  s->frame_pending = true;
  s->cv.notify_all();
}

void render_one(Spike* s) {
  uint64_t flags = mpv_render_context_update(s->rctx);
  if ((flags & MPV_RENDER_UPDATE_FRAME) == 0) return;

  int64_t w = 0, h = 0;
  mpv_get_property(s->mpv, "dwidth", MPV_FORMAT_INT64, &w);
  mpv_get_property(s->mpv, "dheight", MPV_FORMAT_INT64, &h);
  if (w <= 0 || h <= 0) {
    w = 1280;
    h = 720;
  }

  auto t0 = Clock::now();
  {
    std::lock_guard<std::mutex> lock(s->tex_mutex);
    if (s->tex == 0 || w != s->tex_w || h != s->tex_h) {
      if (s->tex == 0) {
        glGenTextures(1, &s->tex);
        glGenFramebuffers(1, &s->fbo);
      }
      glBindTexture(GL_TEXTURE_2D, s->tex);
      glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
      glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
      glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA,
                   GL_UNSIGNED_BYTE, nullptr);
      glBindTexture(GL_TEXTURE_2D, 0);
      glBindFramebuffer(GL_FRAMEBUFFER, s->fbo);
      glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0,
                             GL_TEXTURE_2D, s->tex, 0);
      glBindFramebuffer(GL_FRAMEBUFFER, 0);
      s->tex_w = static_cast<int>(w);
      s->tex_h = static_cast<int>(h);
    }
    mpv_opengl_fbo fbo{static_cast<int>(s->fbo), s->tex_w, s->tex_h, 0};
    int flip_y = 0;
    int block = 0;
    mpv_render_param params[] = {
        {MPV_RENDER_PARAM_OPENGL_FBO, &fbo},
        {MPV_RENDER_PARAM_FLIP_Y, &flip_y},
        {MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, &block},
        {MPV_RENDER_PARAM_INVALID, nullptr},
    };
    mpv_render_context_render(s->rctx, params);
    // Single texture + glFinish: the simplest correct cross-context hand-off
    // (a production version would use a small ring of textures + fences).
    glFinish();
  }
  double ms =
      std::chrono::duration<double, std::milli>(Clock::now() - t0).count();
  uint64_t n = s->frames_rendered++;
  s->render_ms_total += ms;
  if (ms > s->render_ms_max) s->render_ms_max = ms;
  if (n == 0) {
    GLubyte px[4] = {};
    glBindFramebuffer(GL_FRAMEBUFFER, s->fbo);
    glReadPixels(s->tex_w / 8, s->tex_h / 2, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, px);
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
    g_printerr("[spike] first frame %dx%d, probe pixel=(%u,%u,%u) glerr=0x%x\n",
               s->tex_w, s->tex_h, px[0], px[1], px[2], glGetError());
  }
  g_idle_add_full(G_PRIORITY_HIGH_IDLE, mark_frame_available, s, nullptr);
}

void render_thread_main(Spike* s) {
  {
    std::unique_lock<std::mutex> lock(s->cv_mutex);
    s->cv.wait(lock, [s] { return s->ctx_ready; });
  }
  eglBindAPI(s->egl_api);
  if (!eglMakeCurrent(s->egl_dpy, s->egl_surface, s->egl_surface, s->egl_ctx)) {
    g_printerr("[spike] eglMakeCurrent failed: 0x%x\n", eglGetError());
    return;
  }
  g_printerr("[spike] mpv GL: %s / %s\n",
             reinterpret_cast<const char*>(glGetString(GL_RENDERER)),
             reinterpret_cast<const char*>(glGetString(GL_VERSION)));

  mpv_opengl_init_params gl_init{get_proc_address, nullptr};
  mpv_render_param params[] = {
      {MPV_RENDER_PARAM_API_TYPE, const_cast<char*>(MPV_RENDER_API_TYPE_OPENGL)},
      {MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &gl_init},
      // A native display handle enables zero-copy hwdec interop (VAAPI->EGL).
      {s->native_display_is_wayland ? MPV_RENDER_PARAM_WL_DISPLAY
                                    : MPV_RENDER_PARAM_X11_DISPLAY,
       s->native_display},
      {MPV_RENDER_PARAM_INVALID, nullptr},
  };
  if (mpv_render_context_create(&s->rctx, s->mpv, params) < 0) {
    g_printerr("[spike] mpv_render_context_create failed\n");
    return;
  }
  mpv_render_context_set_update_callback(s->rctx, on_mpv_render_update, s);
  // vo=libmpv needs the render context before video init.
  const char* cmd[] = {"loadfile", s->path.c_str(), nullptr};
  mpv_command_async(s->mpv, 0, cmd);

  for (;;) {
    {
      std::unique_lock<std::mutex> lock(s->cv_mutex);
      s->cv.wait(lock, [s] { return s->frame_pending; });
      s->frame_pending = false;
    }
    render_one(s);
  }
}

// ---------------------------------------------------------------------------

gboolean drain_events(gpointer data) {
  Spike* s = static_cast<Spike*>(data);
  while (s->mpv != nullptr) {
    mpv_event* ev = mpv_wait_event(s->mpv, 0);
    if (ev->event_id == MPV_EVENT_NONE) break;
    if (ev->event_id == MPV_EVENT_LOG_MESSAGE) {
      auto* msg = static_cast<mpv_event_log_message*>(ev->data);
      g_printerr("[mpv/%s] %s: %s", msg->level, msg->prefix, msg->text);
    } else if (ev->event_id == MPV_EVENT_END_FILE) {
      auto* ef = static_cast<mpv_event_end_file*>(ev->data);
      g_printerr("[spike] end-file reason=%d error=%s\n", ef->reason,
                 mpv_error_string(ef->error));
    } else if (ev->event_id == MPV_EVENT_FILE_LOADED) {
      g_printerr("[spike] file-loaded\n");
    }
  }
  return G_SOURCE_REMOVE;
}

void on_mpv_wakeup(void* ctx) { g_idle_add(drain_events, ctx); }

// Returns the Flutter texture id (texture mode) or -1 (native mode).
int64_t open_player(Spike* s, const char* path, const char* mode, int x, int y,
                    int w, int h) {
  s->mode = mode;
  s->path = path;
  // libmpv refuses to start unless LC_NUMERIC is "C" (GTK sets the user locale).
  setlocale(LC_NUMERIC, "C");
  s->mpv = mpv_create();
  mpv_set_option_string(s->mpv, "terminal", "no");
  mpv_set_option_string(s->mpv, "hwdec", env_or("SPIKE_HWDEC", "auto-safe"));
  mpv_set_option_string(s->mpv, "ao", env_or("SPIKE_AO", "pipewire,pulse,alsa,null"));
  mpv_set_option_string(s->mpv, "keep-open", "yes");
  mpv_set_option_string(s->mpv, "loop-file", "inf");
  // Keyboard and mouse stay with Flutter; the overlay drives mpv via commands.
  mpv_set_option_string(s->mpv, "input-default-bindings", "no");
  mpv_set_option_string(s->mpv, "input-vo-keyboard", "no");
  mpv_set_option_string(s->mpv, "input-cursor", "no");
  mpv_set_option_string(s->mpv, "osc", "no");
  mpv_request_log_messages(s->mpv, "warn");

  const bool native = std::strcmp(mode, "native") == 0;
  if (native) {
    mpv_set_option_string(s->mpv, "vo", env_or("SPIKE_VO", "gpu-next"));
    mpv_set_option_string(s->mpv, "force-window", "yes");
#ifdef GDK_WINDOWING_X11
    if (!is_wayland()) {
      GdkWindow* parent = gtk_widget_get_window(GTK_WIDGET(s->window));
      Display* xd = GDK_DISPLAY_XDISPLAY(gdk_display_get_default());
      int64_t wid = 0;
      if (g_strcmp0(g_getenv("SPIKE_X11_GDK_CHILD"), "1") == 0) {
        // Pitfall variant: a GDK-managed native child. mpv embeds and plays,
        // but GTK's repaint of the toplevel overdraws it (verified on Xvfb
        // and XWayland): the video is never visible.
        GdkWindowAttr attrs = {};
        attrs.x = x;
        attrs.y = y;
        attrs.width = w;
        attrs.height = h;
        attrs.wclass = GDK_INPUT_OUTPUT;
        attrs.window_type = GDK_WINDOW_CHILD;
        s->child = gdk_window_new(parent, &attrs, GDK_WA_X | GDK_WA_Y);
        gdk_window_ensure_native(s->child);
        gdk_window_show(s->child);
        wid = static_cast<int64_t>(GDK_WINDOW_XID(s->child));
      } else {
        // Working variant: a plain Xlib child unknown to GDK, stacked above
        // the Flutter view. Covers the view; Flutter cannot draw over it.
        Window raw = XCreateSimpleWindow(xd, GDK_WINDOW_XID(parent), x, y, w,
                                         h, 0, 0, 0);
        XMapRaised(xd, raw);
        XFlush(xd);
        wid = static_cast<int64_t>(raw);
      }
      mpv_set_option(s->mpv, "wid", MPV_FORMAT_INT64, &wid);
      // Without this mpv auto-probes Wayland first whenever WAYLAND_DISPLAY
      // is set (XWayland session) and silently opens its own Wayland window,
      // ignoring --wid.
      mpv_set_option_string(s->mpv, "gpu-context", "x11vk,x11egl");
    }
#endif
    // Wayland: no --wid support in libmpv -> mpv opens its own toplevel.
  } else {
    mpv_set_option_string(s->mpv, "vo", "libmpv");
  }

  // Extra overrides for experiments: SPIKE_MPV_OPTS="key=value;key=value".
  gchar** extra = g_strsplit(env_or("SPIKE_MPV_OPTS", ""), ";", -1);
  for (gchar** kv = extra; *kv != nullptr; kv++) {
    gchar** pair = g_strsplit(*kv, "=", 2);
    if (pair[0] != nullptr && pair[1] != nullptr) {
      mpv_set_option_string(s->mpv, pair[0], pair[1]);
    }
    g_strfreev(pair);
  }
  g_strfreev(extra);

  if (mpv_initialize(s->mpv) < 0) {
    g_printerr("[spike] mpv_initialize failed\n");
    return -2;
  }
  mpv_set_wakeup_callback(s->mpv, on_mpv_wakeup, s);
  s->opened_at = Clock::now();

  if (native) {
    const char* cmd[] = {"loadfile", path, nullptr};
    mpv_command(s->mpv, cmd);
    return -1;
  }

#ifdef GDK_WINDOWING_WAYLAND
  if (is_wayland()) {
    s->native_display =
        gdk_wayland_display_get_wl_display(gdk_display_get_default());
    s->native_display_is_wayland = true;
  }
#endif
#ifdef GDK_WINDOWING_X11
  if (!is_wayland()) {
    s->native_display = GDK_DISPLAY_XDISPLAY(gdk_display_get_default());
  }
#endif
  // The render context is created on the render thread once Flutter's EGL
  // context is known (first populate()); loadfile happens there as well.
  s->render_thread = std::thread(render_thread_main, s);
  s->render_thread.detach();
  s->texture = FL_TEXTURE(g_object_new(mpv_gl_texture_get_type(), nullptr));
  fl_texture_registrar_register_texture(s->registrar, s->texture);
  fl_texture_registrar_mark_texture_frame_available(s->registrar, s->texture);
  return fl_texture_get_id(s->texture);
}

void add_double(FlValue* map, mpv_handle* mpv, const char* prop) {
  double v = 0;
  if (mpv_get_property(mpv, prop, MPV_FORMAT_DOUBLE, &v) >= 0) {
    fl_value_set_string_take(map, prop, fl_value_new_float(v));
  }
}

void add_string(FlValue* map, mpv_handle* mpv, const char* prop) {
  char* v = mpv_get_property_string(mpv, prop);
  if (v != nullptr) {
    fl_value_set_string_take(map, prop, fl_value_new_string(v));
    mpv_free(v);
  }
}

FlValue* collect_stats(Spike* s) {
  FlValue* m = fl_value_new_map();
  fl_value_set_string_take(m, "mode", fl_value_new_string(s->mode.c_str()));
  fl_value_set_string_take(m, "gdk_backend",
                           fl_value_new_string(is_wayland() ? "wayland" : "x11"));
  if (s->mpv != nullptr) {
    for (const char* p : {"time-pos", "estimated-vf-fps", "container-fps",
                          "display-fps", "frame-drop-count",
                          "decoder-frame-drop-count", "vo-delayed-frame-count",
                          "mistimed-frame-count", "avsync"}) {
      add_double(m, s->mpv, p);
    }
    for (const char* p : {"current-vo", "current-gpu-context", "hwdec-current",
                          "video-codec", "video-params/pixelformat",
                          "video-params/gamma", "video-params/primaries",
                          "video-out-params/gamma", "dwidth", "dheight",
                          "mpv-version", "ffmpeg-version"}) {
      add_string(m, s->mpv, p);
    }
  }
  fl_value_set_string_take(m, "frames_rendered",
                           fl_value_new_int(static_cast<int64_t>(s->frames_rendered.load())));
  fl_value_set_string_take(m, "populate_calls",
                           fl_value_new_int(static_cast<int64_t>(s->populate_calls.load())));
  uint64_t n = s->frames_rendered.load();
  fl_value_set_string_take(m, "render_ms_avg",
                           fl_value_new_float(n ? s->render_ms_total / n : 0));
  fl_value_set_string_take(m, "render_ms_max", fl_value_new_float(s->render_ms_max));

  struct rusage ru;
  getrusage(RUSAGE_SELF, &ru);
  double cpu = ru.ru_utime.tv_sec + ru.ru_utime.tv_usec / 1e6 + ru.ru_stime.tv_sec +
               ru.ru_stime.tv_usec / 1e6;
  fl_value_set_string_take(m, "process_cpu_s", fl_value_new_float(cpu));
  fl_value_set_string_take(m, "max_rss_kb", fl_value_new_int(ru.ru_maxrss));
  fl_value_set_string_take(
      m, "wall_s",
      fl_value_new_float(std::chrono::duration<double>(Clock::now() - s->opened_at).count()));
  return m;
}

void method_call_cb(FlMethodChannel* channel, FlMethodCall* call, gpointer data) {
  Spike* s = static_cast<Spike*>(data);
  const gchar* method = fl_method_call_get_name(call);
  FlValue* args = fl_method_call_get_args(call);
  g_autoptr(FlMethodResponse) response = nullptr;

  auto arg_str = [&](const char* k) -> const char* {
    FlValue* v = fl_value_lookup_string(args, k);
    return v ? fl_value_get_string(v) : "";
  };
  auto arg_int = [&](const char* k) -> int {
    FlValue* v = fl_value_lookup_string(args, k);
    return v ? static_cast<int>(fl_value_get_int(v)) : 0;
  };

  if (strcmp(method, "open") == 0) {
    int64_t id = open_player(s, arg_str("path"), arg_str("mode"), arg_int("x"),
                             arg_int("y"), arg_int("w"), arg_int("h"));
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(fl_value_new_int(id)));
  } else if (strcmp(method, "command") == 0) {
    // e.g. ["cycle", "pause"] or ["seek", "10"]
    FlValue* list = fl_value_lookup_string(args, "args");
    size_t n = fl_value_get_length(list);
    const char* cmd[16] = {};
    for (size_t i = 0; i < n && i < 15; i++) {
      cmd[i] = fl_value_get_string(fl_value_get_list_value(list, i));
    }
    int rc = s->mpv ? mpv_command(s->mpv, cmd) : -1;
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(fl_value_new_int(rc)));
  } else if (strcmp(method, "stats") == 0) {
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(collect_stats(s)));
  } else {
    response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
  }
  fl_method_call_respond(call, response, nullptr);
}

}  // namespace

void mpv_spike_register(FlView* view, GtkWindow* window) {
  g_spike = new Spike();
  g_spike->view = view;
  g_spike->window = window;
  FlEngine* engine = fl_view_get_engine(view);
  g_spike->registrar = fl_engine_get_texture_registrar(engine);
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  g_spike->channel = fl_method_channel_new(fl_engine_get_binary_messenger(engine),
                                           "pa.spike/mpv", FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(g_spike->channel, method_call_cb,
                                            g_spike, nullptr);
}
