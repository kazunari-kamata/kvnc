# Changelog

## Unreleased

- Include the project MIT license and generated third-party license notices in every binary archive.
- Add a locked dependency-license policy and CI verification for the generated notice.

## 0.1.0

- Initial CLI RFB 3.8 server.
- Hextile/Raw framebuffer fallback and UTF-8 legacy / Extended Clipboard text synchronization.
- Optional RA2ne username/password authentication with encrypted credential exchange and Argon2id verifier storage.
- RFB 3.3 None-security handshake compatibility.
- Optional keyboard and pointer injection with `--allow-input`, including macOS physical-key injection for JIS/IME compatibility.
- Per-monitor capture through `--list-monitors` and `--monitor <INDEX>`.
- Debug logging, background operation, and protocol verification guidance.
- Native standalone binary installation with `cargo install`.
- macOS, Windows, and Linux build/release automation.
- Per-user `--init`, backup-preserving `--update-config`, and clean-checkout `--update` / `--upgrade` maintenance commands.
- Windows clone, PowerShell, desktop-session, and maintenance documentation.
