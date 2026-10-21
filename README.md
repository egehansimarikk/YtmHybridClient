<div align="center">

<img src="./.github/readme-icon.svg" alt="" width="60" height="60">

<h1>YTM Hybrid</h1>

<p align="center">
  <a href="https://github.com/egehansimarikk/YtmHybridClient">Repository</a>
  ·
  <a href="https://github.com/egehansimarikk/YtmHybridClient/releases">Releases</a>
  ·
  <a href="https://github.com/egehansimarikk/YtmHybridClient/issues">Issues</a>
</p>

[![Build](https://img.shields.io/github/actions/workflow/status/egehansimarikk/YtmHybridClient/release.yml?style=flat-square&label=build)](https://github.com/egehansimarikk/YtmHybridClient/actions/workflows/release.yml)
[![License](https://img.shields.io/github/license/egehansimarikk/YtmHybridClient?style=flat-square&label=license&cacheSeconds=3600)](./COPYING)

### Native YouTube Music Desktop Client, built with Rust and GPUI

A dedicated, native desktop client for YouTube Music with a focus on performance and user experience.

</div>

> [!IMPORTANT]
> **YTM Hybrid is not a piracy tool.**
>
> YTM Hybrid is not a platform for obtaining or sharing copyrighted material. We will not implement any functions that can be used to export decrypted streams, DRM licenses, content keys, or to convert protected streams into media files.
>
> YTM Hybrid is not designed to circumvent subscriptions or other restrictions put in place by YouTube Music. If the service requires a valid subscription to play back tracks, so will YTM Hybrid.
>
> Features aimed at ripping, downloading, distributing, or gaining access to protected streaming content are out of scope for the project.

## Features

- **YouTube Music streaming** with native ytmusic-rs integration
- **WebView authentication** using SAPISID/3PAPISID cookies
- Gapless playback with audio normalization
- Shuffle mode and sleep timer
- Synced/karaoke lyrics with romanization support
- Scrobbling with LastFM, ListenBrainz, LibreFM, and Maloja
- Discord Rich Presence
- Customizable themes, fonts, icons, transparency, and blur effects
- Local file playback support
- Native performance with GPUI framework
- Cross-platform: macOS, Windows, Linux

## Installation

### Windows

#### Installer

Download and run the [installer](https://github.com/egehansimarikk/YtmHybridClient/releases/latest/download/YTMHybrid-Setup.exe),
or the [ARM installer](https://github.com/egehansimarikk/YtmHybridClient/releases/latest/download/YTMHybrid-Setup-arm64.exe)
on Windows on ARM.

#### Portable

Download the latest `windows-msvc.exe` for your architecture from [Releases](https://github.com/egehansimarikk/YtmHybridClient/releases/latest).

### Linux

#### AppImage

Download the `x86_64` AppImage from the
[latest release](https://github.com/egehansimarikk/YtmHybridClient/releases/latest), make it executable and run it:

```sh
chmod +x ytm-hybrid-*.AppImage
./ytm-hybrid-*.AppImage
```

An `aarch64` build may be published beside it.

### macOS

Download the `.dmg` from the [latest release](https://github.com/egehansimarikk/YtmHybridClient/releases/latest).

After installing:

```sh
xattr -dr com.apple.quarantine /Applications/YTMHybrid.app
```

## Building from Source

### Prerequisites

- Rust toolchain (edition 2024, resolver 3)
- C++ compiler (for Widevine support)
- Git

### Build Steps

```sh
git clone https://github.com/egehansimarikk/YtmHybridClient.git
cd YtmHybridClient
cargo build --release --package ytm-hybrid
```

The binary will be in `target/release/ytm-hybrid` (or `ytm-hybrid.exe` on Windows).

### Nix

```sh
nix develop                    # or direnv allow
cargo run --locked --package ytm-hybrid
```

## Credits

YTM Hybrid is built with the help of some incredible open-source projects, including:

- [Zed](https://github.com/zed-industries/zed) — provides `gpui`, their native Rust rendering stack
- [ytmusic-rs](https://github.com/sonorahq/ytmusic-rs) — YouTube Music API client library
- [Sonora](https://github.com/sonorahq/sonora) — the original multi-provider music client that inspired this project

## License

Copyright (C) 2026 Egehan Şımarık.

YTM Hybrid is free software, released under the [GNU General Public License version 3 or later](COPYING).

YTM Hybrid is an unofficial client and is not affiliated with, endorsed by, or sponsored by YouTube or Google LLC.

The binary also embeds the [Inter](https://github.com/rsms/inter) typeface (SIL Open Font License 1.1) and four interchangeable icon sets:
[Lucide](https://lucide.dev) (ISC), [Iconoir](https://iconoir.com) (MIT),
[Remix Icon](https://remixicon.com) 4.8.0 (Apache 2.0) and the
[Solar](https://www.figma.com/community/file/1166831539721848736) Linear set
(CC BY 4.0, by 480 Design). Each pack keeps its licence beside its files in
`assets/icons`. `THIRD-PARTY.md` lists every bundled dependency.

## Author

Created by [Egehan Şımarık](https://github.com/egehansimarikk) (Proje Lideri & Geliştirici)
