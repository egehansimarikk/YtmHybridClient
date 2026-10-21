# Privacy Policy

**Last updated: October 20, 2026**

## Overview

YTM Hybrid is a desktop application that provides a native client for YouTube Music. This privacy policy explains what data is collected, how it is used, and your rights regarding your data.

## Data Collection

### Local Data

YTM Hybrid stores the following data locally on your device:

- **Authentication tokens**: SAPISID and 3PAPISID cookies for YouTube Music authentication
- **Application settings**: Your preferences for theme, playback settings, and other configuration
- **Playback history**: Local record of played tracks (stored in `state.sqlite`)
- **Library cache**: Downloaded metadata for your library (stored in `cache.sqlite`)

All data is stored locally on your device in the following locations:

- **Windows**: `%APPDATA%\ytm-hybrid\` or `%LOCALAPPDATA%\ytm-hybrid\`
- **macOS**: `~/Library/Application Support/ytm-hybrid/`
- **Linux**: `~/.local/share/ytm-hybrid/` or `$XDG_DATA_HOME/ytm-hybrid/`

### Network Communication

YTM Hybrid communicates with the following services:

1. **YouTube Music** (music.youtube.com)
   - Purpose: Stream music, fetch library data, authentication
   - Data sent: Authentication cookies, playback requests
   - Data received: Audio streams, metadata, library information

2. **Scrobbling Services** (optional, if enabled)
   - LastFM, ListenBrainz, LibreFM, or Maloja
   - Purpose: Record your listening history
   - Data sent: Track name, artist, album, timestamp
   - Only when explicitly enabled by you

3. **Discord Rich Presence** (optional, if enabled)
   - Purpose: Show "Now Playing" status in Discord
   - Data sent: Current track name, artist, album artwork
   - Only when explicitly enabled by you

4. **GitHub** (for update checks, if enabled)
   - Purpose: Check for application updates
   - Data sent: User agent string with app version
   - Data received: Latest release information
   - Only when update checks are enabled

### Data We Do NOT Collect

- YTM Hybrid does **not** collect any telemetry or analytics
- YTM Hybrid does **not** send any data to the developer or third parties
- YTM Hybrid does **not** track your behavior or usage patterns
- YTM Hybrid does **not** contain any advertising or tracking pixels

## Data Usage

All data collected by YTM Hybrid is used solely for:

- Authenticating with YouTube Music
- Playing back music from your account
- Storing your preferences and settings
- Optional scrobbling to services you explicitly enable
- Optional Discord Rich Presence if you enable it

## Data Sharing

YTM Hybrid does **not** share your data with any third parties except:

- YouTube Music (required for the application to function)
- Optional scrobbling services (only if you enable them)
- Optional Discord Rich Presence (only if you enable it)

## Data Security

- All authentication data is stored locally on your device
- Communication with YouTube Music uses HTTPS encryption
- No data is sent to the developer or any analytics services
- You can delete all local data by removing the application data directory

## Your Rights

You have the right to:

- Access your data (it's stored locally on your device)
- Delete your data (uninstall the application and remove the data directory)
- Export your data (the SQLite databases can be accessed with any SQLite client)
- Disable optional features (scrobbling, Discord Rich Presence, update checks)

## Changes to This Policy

We may update this privacy policy from time to time. Changes will be documented in the CHANGELOG.md file and in new releases of the application.

## Contact

If you have questions about this privacy policy, please open an issue on the [GitHub repository](https://github.com/egehansimarikk/YtmHybridClient/issues).

## Open Source

YTM Hybrid is open-source software. You can review the source code to verify our privacy practices at:
https://github.com/egehansimarikk/YtmHybridClient

## Legal

YTM Hybrid is an unofficial client and is not affiliated with, endorsed by, or sponsored by YouTube or Google LLC.
