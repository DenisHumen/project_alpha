// SPDX-License-Identifier: MPL-2.0
// PLAY-DSK-001 spike: libmpv embedding strategies for the Flutter Linux runner.
#ifndef RUNNER_MPV_SPIKE_H_
#define RUNNER_MPV_SPIKE_H_

#include <flutter_linux/flutter_linux.h>
#include <gtk/gtk.h>

// Registers the "pa.spike/mpv" method channel on |view|. |window| is the
// toplevel that hosts the Flutter view (needed for GL contexts and native
// child windows).
void mpv_spike_register(FlView* view, GtkWindow* window);

#endif  // RUNNER_MPV_SPIKE_H_
