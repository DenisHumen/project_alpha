// SPDX-License-Identifier: MPL-2.0
//
// Runtime licence check for an FFmpeg build: every library must report
// "LGPL version 2.1 or later" and the embedded configure line must not
// enable GPL, nonfree or version3 code. Exit code 0 = OK.
//
// Build: cc check_license.c -I<prefix>/include -L<prefix>/lib \
//          -lavformat -lavcodec -lavfilter -lavdevice -lswresample -lswscale -lavutil

#include <stdio.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavdevice/avdevice.h>
#include <libavfilter/avfilter.h>
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>

static const char *const EXPECTED = "LGPL version 2.1 or later";
static const char *const FORBIDDEN[] = {"--enable-gpl", "--enable-nonfree", "--enable-version3"};

static int check(const char *name, const char *license) {
    int ok = license && strcmp(license, EXPECTED) == 0;
    printf("%-12s %s [%s]\n", name, license ? license : "(null)", ok ? "ok" : "FAIL");
    return ok ? 0 : 1;
}

int main(void) {
    int bad = 0;
    bad |= check("avutil", avutil_license());
    bad |= check("avcodec", avcodec_license());
    bad |= check("avformat", avformat_license());
    bad |= check("avfilter", avfilter_license());
    bad |= check("avdevice", avdevice_license());
    bad |= check("swresample", swresample_license());
    bad |= check("swscale", swscale_license());

    const char *conf = avutil_configuration();
    printf("configuration: %s\n", conf);
    for (size_t i = 0; i < sizeof(FORBIDDEN) / sizeof(FORBIDDEN[0]); i++) {
        if (strstr(conf, FORBIDDEN[i])) {
            printf("forbidden option in configuration: %s [FAIL]\n", FORBIDDEN[i]);
            bad = 1;
        }
    }
    printf("version: %s\n", av_version_info());
    printf("result: %s\n", bad ? "FAIL" : "OK");
    return bad;
}
