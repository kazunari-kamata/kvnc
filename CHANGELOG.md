# Changelog

## 0.4.0

- Add `scripts/install-windows.ps1` and its Command Prompt wrapper to check the Rust MSVC toolchain, make a locked release build, deploy `bin\kvnc.exe`, and safely initialize a missing per-user configuration without starting kVNC or overwriting an existing configuration.
- Replace macOS desktop snapshots with a live CoreGraphics `CGDisplayStream`, so compositor changes such as application windows and menu popovers continue to reach VNC clients after a display wake.
- Add `all_monitors` / `--all-monitors` to compose every available display into one position-preserving virtual-desktop VNC framebuffer, with per-display pointer routing.
- Add safe `scripts/kvncctl` and `scripts/kvncctl.ps1` helpers for managed start, stop, restart, status, and logs; document LaunchAgent `caffeinate` lifecycle and parallel development instances.
- Add `scripts/kvncctl.cmd` as a Command Prompt launcher for the reviewed Windows PowerShell controller.
- Support safe `--update` operation in clean JJ checkouts as well as standard Git clones, without leaving Cargo installation metadata in the checkout.
- Use a one-time macOS LaunchAgent startup wake before desktop capture, then permit normal display sleep while retaining idle-system-sleep inhibition.
- Add opt-in platform-native display wake on a successful desktop VNC connection through `wake_display_on_connect`: macOS wakes then holds the display only for the authenticated client lifetime, Windows uses an execution state, and Linux uses X11 DPMS or Wayland session-idle inhibition.
- Treat an empty or non-text local clipboard as normal, preventing repeated error-log entries while preserving the last synchronized text clipboard value.
- Reacquire the selected desktop monitor after a display wake or reconfiguration, use current framebuffer geometry for pointer injection, and log the selected monitor identity and geometry.
- On macOS, complete the short `caffeinate -u -t 5` recovery before acquiring the active-client display assertion or processing framebuffer requests. This matches the documented manual SSH recovery sequence while still allowing display sleep when no client is connected.

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
