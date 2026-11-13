<!-- SPDX-License-Identifier: MPL-2.0 -->

# project_alpha — Kinolune

[![Rust](https://github.com/DenisHumen/project_alpha/actions/workflows/rust.yml/badge.svg)](https://github.com/DenisHumen/project_alpha/actions/workflows/rust.yml)
[![Hygiene](https://github.com/DenisHumen/project_alpha/actions/workflows/hygiene.yml/badge.svg)](https://github.com/DenisHumen/project_alpha/actions/workflows/hygiene.yml)
[![DCO](https://github.com/DenisHumen/project_alpha/actions/workflows/dco.yml/badge.svg)](https://github.com/DenisHumen/project_alpha/actions/workflows/dco.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](LICENSE)

[Русский](README.md) · **English**

A free and open source media player for every platform: Linux, macOS, Windows, iOS/iPadOS, Android/Android TV and Apple TV. The working product name is **Kinolune** (ADR-0011); the repository is still called `project_alpha`.

- a library of movies, TV shows and anime with accurate recognition from file and folder names;
- playback of almost any format, HDR/Dolby Vision, audio passthrough;
- sources: local folders, SMB, NFS, WebDAV, Plex/Jellyfin/Emby, our own media server;
- a music player (mp3, flac, alac, opus, dsd…), gapless playback, lyrics;
- a comfortable Twitch client: live streams with chat, VODs, favourite streamers;
- continue watching on any device, with no cloud: everything stays local;
- our own NAS Linux distribution with a simple installer and RAID.

The project runs no servers and has no accounts: the app runs on your devices, requests to TMDB and other services go directly from your device, and there is no telemetry. The project is funded by voluntary donations.

## Status

🚧 **Early development, no builds yet.** We are working on milestone M0 (repository skeleton, CI, Rust core) and preparing **release 0.1** for Linux desktop and Android: local folders and SMB, recognition, a library with posters, the player, "Continue watching" and a basic music player ([ADR-0011](docs/adr/0011-maintainer-decisions-release-0-1.md), [0.1 contract](docs/dev/release-0.1-contract.md); documents are in Russian).

Current progress: [docs/STATUS.md](docs/STATUS.md); the full plan: [docs/ROADMAP.md](docs/ROADMAP.md).

Screenshots will come with the first release (our own material and open test media only, no third-party artwork).

## Download

Builds will be published in this repository's [GitHub Releases](https://github.com/DenisHumen/project_alpha/releases). The first release is 0.1 (Linux: tar.gz and AppImage; Android: APK).

## Support the project

Donation platforms are still being set up; once they are live, links will appear here and on the repository's **Sponsor** button. Bug reports, wrong-match examples and pull requests already help a lot.

## Documentation

Most documentation is in Russian for now; user-facing docs will be bilingual.

- [docs/STRATEGY.md](docs/STRATEGY.md): strategy and key decisions
- [docs/ROADMAP.md](docs/ROADMAP.md): detailed plan and task checklist
- [docs/adr/](docs/adr/): architecture decision records
- [docs/dev/architecture.md](docs/dev/architecture.md): architecture for developers
- [docs/research/](docs/research/): research notes
- [docs/legal/privacy-policy.md](docs/legal/privacy-policy.md): privacy policy

## Contributing

Pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md) (English summary at the end) and the [Code of Conduct](CODE_OF_CONDUCT.md). AI agents: see [AGENTS.md](AGENTS.md). Questions and ideas go to [Discussions](https://github.com/DenisHumen/project_alpha/discussions). Report vulnerabilities privately only, see [SECURITY.md](SECURITY.md).

## License

Code is licensed under [MPL-2.0](LICENSE). The repository follows the [REUSE](https://reuse.software/) specification: each file's licence is in its header or in [REUSE.toml](REUSE.toml), licence texts are in [LICENSES/](LICENSES/).

Kinolune is not affiliated with Infuse/Firecore, Plex, Jellyfin, Emby, Twitch, TMDB or any other service mentioned; their names belong to their owners.
