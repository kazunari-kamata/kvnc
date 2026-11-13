# Changelog

## Unreleased

## 0.3.0

- Keep the authenticated desktop server available in view-only mode when local input injection permission is unavailable.
- Prevent macOS LaunchAgent desktop capture from failing when displays sleep by running the agent through `caffeinate`.
- Add Windows interactive logon-task and power-policy guidance, plus Linux graphical `systemd --user` / optional `systemd-inhibit` guidance with X11 and Wayland boundaries.
- Keep locally generated runtime logs under `var/` out of Git tracking.

## 0.2.0

- Include the project MIT license and generated third-party license notices in every binary archive.
- Add a locked dependency-license policy and CI verification for the generated notice.
- Build and verify macOS arm64 artifacts on GitHub's supported `macos-15` runner.
- Correct the installation security guidance for optional RA2ne authentication.

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
