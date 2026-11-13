// SPDX-License-Identifier: MPL-2.0
//
// Runtime check for a libmpv build (PLAY-002): mpv_client_api_version() is
// exported and matches the headers, a headless player can be created and
// initialised, and its build configuration says -Dgpl=false. Exit code 0 = OK.
//
// Build: cc check_libmpv.c -I<prefix>/include -L<prefix>/lib -lmpv

#include <stdio.h>
#include <string.h>

#include <mpv/client.h>

static int failures = 0;

static void expect(int cond, const char *what) {
    printf("%-48s [%s]\n", what, cond ? "ok" : "FAIL");
    if (!cond)
        failures++;
}

int main(void) {
    unsigned long v = mpv_client_api_version();
    printf("mpv_client_api_version() = %lu.%lu (headers %lu.%lu)\n", v >> 16, v & 0xffff,
           (unsigned long)MPV_CLIENT_API_VERSION >> 16, (unsigned long)MPV_CLIENT_API_VERSION & 0xffff);
    expect(v >> 16 == MPV_CLIENT_API_VERSION >> 16, "client API major version matches headers");
    expect(v >= MPV_CLIENT_API_VERSION, "client API version >= headers");

    mpv_handle *mpv = mpv_create();
    expect(mpv != NULL, "mpv_create()");
    if (!mpv)
        return 1;
    // Headless: no window, no audio device, no config files.
    mpv_set_option_string(mpv, "vo", "null");
    mpv_set_option_string(mpv, "ao", "null");
    mpv_set_option_string(mpv, "config", "no");
    mpv_set_option_string(mpv, "terminal", "no");
    expect(mpv_initialize(mpv) >= 0, "mpv_initialize()");

    char *conf = mpv_get_property_string(mpv, "mpv-configuration");
    printf("mpv-configuration: %s\n", conf ? conf : "(null)");
    expect(conf && strstr(conf, "-Dgpl=false") != NULL, "configuration contains -Dgpl=false");
    expect(conf && strstr(conf, "-Dgpl=true") == NULL, "configuration has no -Dgpl=true");
    mpv_free(conf);

    char *ff = mpv_get_property_string(mpv, "ffmpeg-version");
    printf("ffmpeg-version: %s\n", ff ? ff : "(null)");
    expect(ff != NULL, "ffmpeg-version property");
    mpv_free(ff);

    mpv_terminate_destroy(mpv);
    return failures ? 1 : 0;
}
