# YTM Hybrid - Claude Code Project Guide

## Project Overview

YTM Hybrid is a native YouTube Music desktop client built with Rust and GPUI framework. This is a cross-platform application supporting Windows, Linux, and macOS.

**Current Version:** 0.2.0  
**License:** GPL-3.0-or-later  
**Rust Edition:** 2024  
**Workspace Resolver:** 3

## Project Structure

This is a Cargo workspace with multiple crates:

```
crates/
├── embed/       # Asset embedding
├── i18n/        # Internationalization
├── icons/       # Icon sets (Lucide, Iconoir, Remix, Solar)
├── input/       # Input handling
├── music/       # YouTube Music API integration (ytmusic-rs)
├── router/      # Navigation and routing
├── sonora/      # Main application crate
├── state/       # Application state management
├── storage/     # Data persistence
├── ui/          # UI components (GPUI-based)
├── views/       # Application views/screens
├── webview/     # WebView integration for auth
└── widevine/    # Widevine CDM support
```

### Dependency Hierarchy

**Strict dependency order (never create circular dependencies):**

```
sonora → views → state → music
         state, music → storage
         state → webview
         music → widevine
         all ui-side crates → ui, router, input → ui → gpui
         every ui-side crate → i18n, icons → gpui
```

## Build Commands

### Development
```bash
cargo run --package ytm-hybrid              # Run in debug mode
cargo run --locked --package ytm-hybrid     # Run with locked dependencies
```

### Building
```bash
cargo build --release --package ytm-hybrid  # Release build
cargo build --workspace                     # Build all crates
```

### Code Quality
```bash
cargo fmt                                   # Format code
cargo fmt --check                           # Check formatting
cargo clippy --workspace --all-targets      # Lint all targets
cargo clippy -- -D warnings                 # Lint with warnings as errors
cargo check --workspace                     # Fast compilation check
cargo test --workspace                      # Run all tests
```

### Packaging
```bash
./package.sh                                # Package for current platform
```

### Documentation
```bash
cargo doc --no-deps --open                  # Generate and open docs
```

### Nix Development
```bash
nix develop                                 # Enter Nix shell
direnv allow                                # Auto-enter with direnv
```

## Code Style & Conventions

### Formatting
- **Always run `cargo fmt` before committing**
- Configuration in `rustfmt.toml`
- Use `rustfmt.toml` settings for consistent formatting

### Linting
- Zero clippy warnings policy: `cargo clippy -- -D warnings`
- All warnings must be addressed before merging
- Use `#[allow(clippy::lint_name)]` sparingly and only with justification

### Commit Messages
Follow Conventional Commits format:
```
type(scope): description

Examples:
- feat(player): add volume normalization
- fix(ui): correct theme switching bug
- docs(readme): update installation instructions
- refactor(music): simplify API client code
- chore(deps): update gpui to latest commit
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `perf`  
**Scopes:** `views`, `ui`, `music`, `state`, `playback`, `player`, `settings`, `router`, `local`, `webview`, `widevine`, `i18n`, `icons`

### Testing
- Write tests for new functionality
- Run `cargo test --workspace` before committing
- Integration tests should be in separate files
- Unit tests in the same file as the code they test

## Key Technologies

### Core Stack
- **Language:** Rust (edition 2024)
- **UI Framework:** GPUI (from Zed editor)
- **Music API:** ytmusic-rs (YouTube Music integration)
- **WebView:** Platform-native webview for authentication
- **Audio:** rodio, cpal, symphonia
- **Streaming:** librespot (Spotify integration)

### Platform-Specific
- **Linux:** ALSA, Wayland, X11, D-Bus (MPRIS, system tray)
- **macOS:** Objective-C bindings (objc2), WebKit
- **Windows:** Windows API, WebView2

### Notable Dependencies
- **Async Runtime:** tokio
- **HTTP Client:** reqwest (with rustls)
- **Database:** rusqlite
- **Serialization:** serde, serde_json
- **Caching:** moka
- **Rich Presence:** discord-rich-presence
- **Media Control:** souvlaki (media keys)

## Features

### Implemented
- YouTube Music streaming with native API integration
- WebView-based authentication (SAPISID/3PAPISID cookies)
- Gapless playback with audio normalization
- Shuffle mode and sleep timer
- Synced/karaoke lyrics with romanization support
- Scrobbling: Last.fm, ListenBrainz, LibreFM, Maloja
- Discord Rich Presence
- Customizable themes, fonts, icons, transparency, blur effects
- Local file playback
- MPRIS support (Linux)
- System tray integration
- Cross-platform media key support

### Anti-Features (Do Not Implement)
**YTM Hybrid is NOT a piracy tool:**
- No DRM circumvention
- No stream ripping or downloading
- No export of decrypted content or keys
- No subscription bypass mechanisms
- Features must respect YouTube Music's terms of service

## Development Workflow

### Before Starting Work
1. Pull latest changes: `git pull origin main`
2. Create feature branch: `git checkout -b feat/your-feature`
3. Ensure dependencies are up to date: `cargo update` (if needed)

### During Development
1. Make incremental commits
2. Run `cargo check` frequently for fast feedback
3. Use `cargo clippy` to catch common mistakes
4. Add tests for new functionality
5. Update documentation as needed

### Before Committing
1. Format code: `cargo fmt`
2. Check lints: `cargo clippy --workspace --all-targets`
3. Run tests: `cargo test --workspace`
4. Verify builds: `cargo build --workspace`
5. Test the binary: `cargo run --package ytm-hybrid`

### Pull Request Checklist
- [ ] Code formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy -- -D warnings`)
- [ ] Tests pass (`cargo test --workspace`)
- [ ] Builds successfully (`cargo build --release`)
- [ ] Commit messages follow convention
- [ ] Documentation updated
- [ ] No breaking changes (or clearly documented)
- [ ] Respects project anti-piracy policy

## Common Tasks

### Adding a New Crate
1. Create under `crates/`
2. Add to workspace members in root `Cargo.toml`
3. Add workspace dependency entry if needed
4. Follow dependency hierarchy rules
5. Update this documentation

### Adding a New Dependency
1. Add to `[workspace.dependencies]` in root `Cargo.toml`
2. Reference in crate's `Cargo.toml` with `dependency = { workspace = true }`
3. Prefer `default-features = false` and explicit feature selection
4. Use `rustls` over `native-tls` when available
5. Document why the dependency is needed

### Working with GPUI
- GPUI is a native Rust UI framework from Zed
- Uses a reactive model similar to SwiftUI/React
- Custom fork: `git = "https://github.com/sonorahq/gpui"`
- Render performance is critical - avoid blocking the UI thread
- Use `cx` (context) for state management and rendering

### Internationalization (i18n)
- Translations in `crates/i18n/`
- Uses Fluent format
- Add new strings to all language files
- Use `i18n!` macro for translated strings
- System locale auto-detection with fallback to English

### Icon Management
- Four icon sets available: Lucide, Iconoir, Remix, Solar
- SVG icons embedded at compile time
- User can switch between sets in settings
- Keep licenses with icon files in `assets/icons/`

## Performance Considerations

### Release Profile
```toml
[profile.release]
lto = "thin"           # Link-time optimization
codegen-units = 1      # Single codegen unit for better optimization
strip = "debuginfo"    # Remove debug info from binary
```

### Development Profile
- Split debuginfo for faster incremental builds
- Optimized dependencies (`opt-level = 2`) for better dev experience
- UI and views crates optimized even in dev mode

## Platform-Specific Notes

### Linux
- Requires ALSA development libraries
- Wayland and X11 support through GPUI
- D-Bus for MPRIS and system integration
- System tray via ksni

### Windows
- Requires WebView2 runtime
- Windows 10+ required for best experience
- Widevine CDM for protected content
- MSI installer available

### macOS
- Currently no official builds (planned)
- Gatekeeper quarantine: `xattr -dr com.apple.quarantine /Applications/YTMHybrid.app`
- Native WebKit for authentication
- Objective-C bridge for system integration

## Troubleshooting

### Build Issues
- **Error with GPUI:** Ensure git submodules are updated
- **Linking errors:** Check platform-specific dependencies are installed
- **Widevine issues:** Ensure C++ compiler is available

### Runtime Issues
- **Audio not working:** Check audio backend (ALSA on Linux, default on others)
- **Login fails:** Clear WebView cache, check internet connection
- **High CPU usage:** Check for infinite loops in UI updates

## Resources

- **Main Repository:** https://github.com/egehansimarikk/YtmHybridClient
- **GPUI Documentation:** https://github.com/zed-industries/zed
- **ytmusic-rs:** https://github.com/sonorahq/ytmusic-rs
- **Sonora (inspiration):** https://github.com/sonorahq/sonora

## AI Usage Policy

We use AI tools in development, but:
- You must understand and review all AI-generated code
- Test everything thoroughly before committing
- Communication must be from real people, not bots
- PR/issue descriptions should reflect your understanding
- AI-assisted proofreading and translation are OK

PRs that blindly include AI-generated code without understanding may be rejected.

## License & Credits

**Copyright (C) 2026 Egehan Şımarık**

Licensed under GNU General Public License v3.0 or later.

### Third-Party Components
- **Inter Font:** SIL Open Font License 1.1
- **Lucide Icons:** ISC License
- **Iconoir:** MIT License
- **Remix Icon 4.8.0:** Apache 2.0 License
- **Solar Icons:** CC BY 4.0 (by 480 Design)

See `THIRD-PARTY.md` for complete dependency licenses.

## Project Status

Active development. See `CHANGELOG.md` for recent changes.

**Current Focus:**
- Stability improvements
- Performance optimization
- macOS official builds
- Additional streaming service integrations

## Contact

**Author:** Egehan Şımarık  
**GitHub:** @egehansimarikk

For questions or issues, please use GitHub Issues.
