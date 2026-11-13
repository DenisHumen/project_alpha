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
libmpv and FFmpeg. Only LGPL builds are used; no GPL or nonfree code is shipped.

- **Linux** (tar.gz, AppImage): libmpv and FFmpeg are **not** included; the
  system libraries are used (`libmpv2` / `mpv-libs` / `mpv`).
- **Android** (APK): the APK contains a prebuilt `libmpv.so` (mpv + FFmpeg and
  their dependencies linked into one shared library) from
  [libmpv-android-video-build](https://github.com/media-kit/libmpv-android-video-build).
  The exact versions, licences and the corresponding-source tarball are listed
  in the release notes and attached to the same GitHub Release
  (`kinolune-<version>-android-lgpl-sources.tar`). You may replace `libmpv.so`
  in the APK with your own build and re-sign the APK.

Licence texts for these components are in `licenses/`:

| File | Used by |
|---|---|
| `LGPL-2.1-or-later.txt` | mpv (built with `-Dgpl=false`), GNU FriBidi |
| `LGPL-3.0-or-later.txt`, `GPL-3.0-or-later.txt` | FFmpeg in the Android build (`--enable-version3`); LGPL-3.0 incorporates GPL-3.0 by reference |
| `Apache-2.0.txt` | Mbed TLS |
| `BSD-2-Clause.txt` | dav1d |
| `ISC.txt` | libass |
| `MIT.txt` | HarfBuzz, libxml2 |
| `FTL.txt` | FreeType |

## Metadata

This product uses the TMDB API but is not endorsed or certified by TMDB.
