<!-- SPDX-License-Identifier: MPL-2.0 -->
# Kinolune — third-party notices

Kinolune is licensed under the Mozilla Public License 2.0 (`LICENSE`).
Source code: https://github.com/DenisHumen/project_alpha

## Flutter and Dart packages

The Flutter engine, framework and every Dart package compiled into the app list
their licences in `licenses/FLUTTER-NOTICES.txt` (Linux) and on the
“Licences” page inside the app (all platforms).

## Media playback (libmpv, FFmpeg)

Playback uses [media_kit](https://github.com/media-kit/media-kit) on top of
libmpv and FFmpeg. The app ships our own LGPL build of both, made from the
recipes in [`tools/mpv`](https://github.com/DenisHumen/project_alpha/tree/main/tools/mpv)
and [`tools/ffmpeg`](https://github.com/DenisHumen/project_alpha/tree/main/tools/ffmpeg):
mpv configured with `-Dgpl=false`, FFmpeg licensed LGPL-2.1-or-later (no GPL,
nonfree or version3 code), each as separate shared libraries.

- **Android** (APK): `lib/<abi>/libmpv.so`, `libavcodec.so`, `libavfilter.so`,
  `libavformat.so`, `libavutil.so`, `libswresample.so`, `libswscale.so` and the
  media_kit JNI helper `libmediakitandroidhelper.so` (MIT).
- **Linux** (tar.gz, AppImage): `lib/libmpv.so.2` and the FFmpeg libraries next
  to the app; their licence texts and build record are in `licenses/libmpv/`.

The versions and licences of every component are listed in the release notes
and in `LIBMPV-COMPONENTS.md`. The corresponding source (all upstream tarballs,
build recipes and patches) is attached to the same GitHub Release
(`kinolune-<version>-libmpv-sources.tar.xz`). You may rebuild the libraries and
replace them: in the APK (then re-sign it with your own key) or in the Linux
bundle's `lib/` directory.

Licence texts for these components are in `licenses/` (SPDX texts) and
`licenses/libmpv/` (the upstream files):

| File | Used by |
|---|---|
| `LGPL-2.1-or-later.txt` | mpv (built with `-Dgpl=false`), libplacebo, FFmpeg, GNU FriBidi |
| `BSD-2-Clause.txt` | dav1d |
| `ISC.txt` | libass |
| `MIT.txt` | HarfBuzz, fast_float, glad, media_kit JNI helper, libdisplay-info (Linux) |
| `Apache-2.0.txt` | Vulkan-Headers (declarations only), glad-generated loader |
| `FTL.txt` | FreeType |

## Metadata

This product uses the TMDB API but is not endorsed or certified by TMDB.
