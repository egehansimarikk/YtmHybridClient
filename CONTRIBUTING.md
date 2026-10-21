# Contributing to YTM Hybrid

Thank you for your interest in contributing to YTM Hybrid!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR-USERNAME/YtmHybridClient.git`
3. Create a new branch: `git checkout -b feature/your-feature-name`
4. Make your changes
5. Test your changes: `cargo check --workspace && cargo test --workspace`
6. Commit your changes with a descriptive commit message
7. Push to your fork: `git push origin feature/your-feature-name`
8. Open a Pull Request

## Development Setup

### Prerequisites

- Rust toolchain (edition 2024, resolver 3)
- C++ compiler (for Widevine CDM support)
- Git

### Building

```sh
nix develop                    # or direnv allow (if using Nix)
cargo run --locked --package ytm-hybrid
cargo fmt
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
```

## Code Style

- Follow the existing code style in the project
- Use `cargo fmt` to format your code
- Ensure `cargo clippy` passes with no warnings
- Write descriptive commit messages following Conventional Commits format: `type(scope): description`
- Keep commits focused and atomic

### Commit Message Format

```
type(scope): description

Examples:
- feat(player): add volume normalization
- fix(ui): correct theme switching bug
- docs(readme): update installation instructions
- refactor(music): simplify API client code
```

Common types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

Common scopes: `views`, `ui`, `music`, `state`, `playback`, `player`, `settings`, `router`, `local`

## Pull Request Guidelines

- Keep PRs focused on a single feature or bug fix
- Include tests for new functionality
- Update documentation as needed
- Ensure all CI checks pass
- Write a clear PR description explaining what changes were made and why

### PR Description Format

```markdown
## Summary

- Add feature X
- Fix bug Y
- Update Z

(Bullets are lowercase, imperative, one line each)
```

## Code Architecture

YTM Hybrid follows a strict dependency hierarchy:

```
sonora → views → state → music
         state, music → storage
         state → webview
         music → widevine
         all ui-side crates → ui, router, input → ui → gpui
         every ui-side crate → i18n, icons → gpui
```

See `CLAUDE.md` for detailed architectural guidelines.

## AI Usage Policy

We have nothing against the usage of LLMs in the project — in fact, we use them ourselves. However:

- You must understand, review, and test all AI-generated code
- Communication must be with a real person, not a computer
- PR/issue text generation should reflect your own understanding
- AI-assisted proofreading and translation are permitted

PRs that fail to adhere to these requirements may be rejected without further notice.

## Questions?

Feel free to open an issue for questions or clarifications.

Thank you for contributing to YTM Hybrid!
