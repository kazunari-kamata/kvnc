# Background operation

Japanese: [daemon.ja.md](daemon.ja.md).

## Foreground startup from Terminal

For the first `--source desktop` startup and permission grant, use Terminal.app or iTerm2 opened in the signed-in Mac GUI session. Do not use an SSH shell or a terminal on the remote-client host. With `~/.config/kvnc/config.toml` configured, no arguments are required from any working directory:

```sh
/Users/YOUR_USER/path/to/kvnc/bin/kvnc
```

Stop the foreground process with `Control-C` and run the same command to start it again. Grant macOS Screen Recording and Accessibility permission to this local GUI launch context. Do not use `sudo`.

For safe control of an existing LaunchAgent or daemon, and for a separate development listener, see [Instance control and development workflow](control.en.md).

## Config daemon on macOS and Linux

`daemon = true` in `~/.config/kvnc/config.toml` detaches kVNC from the current terminal. `log_file` is required; `pid_file` identifies the precise stop target.

```toml
# ~/.config/kvnc/config.toml
daemon = true
log_file = "/Users/YOUR_USER/path/to/kvnc/var/kvnc.log"
log_rotation = "daily"
log_max_size_bytes = 104857600
log_retention = 14
pid_file = "/Users/YOUR_USER/path/to/kvnc/var/kvnc.pid"
```

```sh
mkdir -p var
/Users/YOUR_USER/path/to/kvnc/bin/kvnc
```

Check the service and stop it with:

```sh
tail -f var/kvnc.log
kill "$(cat var/kvnc.pid)"
```

Use absolute log-file and pid-file paths in automation. The config daemon is available on macOS and Linux. On Windows, use a logon task that runs only in the signed-in interactive user session; do not use a Windows service for desktop capture.

kVNC owns rotation of `log_file`: it archives the previous file as `kvnc.log.<Unix-timestamp>` and creates a fresh active file at the original path. `log_rotation` accepts `none`, `hourly`, `daily`, `weekly`, and `monthly`; `monthly` is a rolling 30-day interval. A nonzero `log_max_size_bytes` triggers an earlier rotation. `log_retention` controls the number of archives kept. This works for foreground, daemon, and LaunchAgent operation without reopening the process. If an operator or external rotator replaces or removes the active path, kVNC detects it before its next write and reopens the configured path; use `scripts/kvncctl logs` or the configured path rather than relying on a stale file descriptor.

## Linux desktop capture: use a graphical user service

`caffeinate` is a macOS command and is not used on Linux. Run desktop capture in the logged-in graphical user's session, never as a root system service or a lingering headless user service. On systemd-based distributions, the following user unit is a starting point. Replace the paths and configure `~/.config/kvnc/config.toml` first; leave `daemon = false` because systemd owns the process lifecycle.

```ini
# ~/.config/systemd/user/kvnc.service
[Unit]
Description=kVNC desktop server
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/systemd-inhibit --what=idle:sleep --mode=block --why=kVNC /home/YOUR_USER/path/to/kvnc/bin/kvnc
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
```

Create and enable it from a terminal in that graphical session:

```sh
mkdir -p ~/.config/systemd/user
systemctl --user daemon-reload
systemctl --user enable --now kvnc.service
systemctl --user status kvnc.service
```

`systemd-inhibit` asks logind not to perform idle or sleep actions while kVNC is running; it does not change the desktop environment's display blanking, screen-lock, or Wayland portal policy. On X11, desktop capture must see the same `DISPLAY` and X authority as the graphical session. On Wayland, the compositor and screen-capture portal decide whether a user service can capture; run `--source demo` first and then smoke-test the actual desktop session. If the distribution or desktop environment does not provide `graphical-session.target`, use its graphical-session startup facility instead of enabling linger or moving kVNC into a system service.

### Wake the Linux display when a client connects

Set the following in the graphical user's configuration to make a successful
desktop VNC connection request a wake without keeping the display awake:

```toml
wake_display_on_connect = true
```

kVNC selects the active session type, never injects keyboard or pointer input,
and logs the selected action.

- **X11:** kVNC runs `xset s reset` followed by `xset dpms force on` using the
  server process's `DISPLAY` and X authority, then holds session-idle
  inhibition for five seconds when the D-Bus service is available. Install the
  `xset` utility (often in the `x11-xserver-utils` package) and launch kVNC
  from the same graphical session. This resets the normal idle timer; it does
  not disable future DPMS, locking, or power policy.
- **Wayland:** kVNC opens the graphical session's D-Bus connection and holds
  the standard `org.freedesktop.ScreenSaver.Inhibit` request for five seconds,
  then releases it. This covers session idle actions such as dimming and
  software screen savers on supporting compositors. Wayland deliberately has
  no universal client API that can force a physically powered-off monitor on;
  the compositor may require local user activity. kVNC reports an unavailable
  D-Bus service rather than falling back to synthetic input.

Neither path unlocks a locked session or bypasses a display manager. If the
capture backend has already lost every monitor, wake the display locally and
restart kVNC. This setting is only run after a complete RFB handshake (and
RA2ne authentication when configured), never for a raw TCP probe.

Stop or remove this user service with:

```sh
systemctl --user disable --now kvnc.service
rm ~/.config/systemd/user/kvnc.service
systemctl --user daemon-reload
```

## macOS desktop capture: use a LaunchAgent

For `--source desktop`, run as the logged-in user through a **LaunchAgent**, not a root LaunchDaemon and not `sudo`. Screen Recording permission belongs to the signed-in desktop user's process identity; a system daemon cannot use it.

Create `~/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist`, replacing the example paths and the trusted listener address:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.kazunari-kamata.kvnc</string>
  <key>ProgramArguments</key><array>
    <string>/Users/YOUR_USER/path/to/kvnc/scripts/kvnc-launch-macos.sh</string>
  </array>
  <key>WorkingDirectory</key><string>/Users/YOUR_USER/path/to/kvnc</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/Users/YOUR_USER/path/to/kvnc/var/launchd.out.log</string>
  <key>StandardErrorPath</key><string>/Users/YOUR_USER/path/to/kvnc/var/launchd.err.log</string>
</dict></plist>
```

Create the log directory, then load it in the logged-in user's GUI session:

```sh
mkdir -p /Users/YOUR_USER/path/to/kvnc/var
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist"
launchctl kickstart -k "gui/$(id -u)/com.kazunari-kamata.kvnc"
```

The repository wrapper first runs the standalone `/usr/bin/caffeinate -u -t 5` and waits for it to finish, then executes `/usr/bin/caffeinate -i bin/kvnc`. The first, five-second assertion wakes an already-sleeping display before the capture backend enumerates monitors; without it, macOS can report no active monitor and kVNC cannot start desktop capture. The long-lived `-i` assertion prevents idle **system** sleep but deliberately does not prevent normal display sleep: `-d` is intentionally absent. This keeps the display's normal System Settings power timer effective.

The user configuration at `~/.config/kvnc/config.toml` is loaded automatically; `WorkingDirectory` does not select configuration. Do not enable `daemon` in that config when using a LaunchAgent: launchd owns restart and process lifecycle. Permit the standalone `kvnc` binary in **System Settings → Privacy & Security → Screen Recording** (Terminal permission alone is not enough for a LaunchAgent) and, when remote input is enabled, **Accessibility**, then restart the agent after changing either permission. A locally rebuilt or replaced CLI binary can require Accessibility approval again; after a build or `--update`, confirm the log says `local keyboard and pointer input injection enabled`. If it instead reports view-only mode, re-enable the current kVNC launch context under Accessibility and restart the agent. If input permission is unavailable, kVNC stays available in view-only mode and records the reason in its log. `KeepAlive` restarts a killed wrapper, so stop the job with `scripts/kvncctl stop` or `launchctl bootout "gui/$(id -u)/com.kazunari-kamata.kvnc"`; do not kill `caffeinate` directly. See [Instance control and development workflow](control.en.md) for start, status, restart, and development-instance commands.

### Allow display sleep

The standard wrapper above is the power-saving profile: macOS follows its
normal display-sleep timer, configured in System Settings. It does not change
the screen saver or `pmset` settings. Do not add persistent `-d` or `-u` to
the wrapper unless keeping the local panel continuously lit is explicitly
required; doing so defeats this profile. Do not embed `pmset displaysleepnow`
in the LaunchAgent: it forces the no-monitor capture state rather than
preserving a usable service.

### Wake the display when a VNC client connects

When you prefer the power-saving profile but currently wake the Mac from a
separate SSH session with `caffeinate -u -t 5`, enable the equivalent kVNC
setting instead:

```toml
# ~/.config/kvnc/config.toml
wake_display_on_connect = true
```

On macOS with `source = "desktop"`, kVNC runs
`/usr/bin/caffeinate -u -t 5` after a VNC client completes the RFB handshake
and waits for that five-second command to finish before it processes the
client's framebuffer requests. It then holds a separate `caffeinate -d`
assertion for that authenticated client's connection lifetime. The assertion is released when the client disconnects, so
the normal macOS display-sleep timer remains effective whenever no VNC client
is connected. This is intended to keep the composited desktop capture live
after display wake instead of serving a stale pre-sleep frame. When RA2ne authentication is
configured, this happens after the username and password have been accepted;
it does not run for a bare TCP connection. The setting is disabled by default
because a successful connection can wake the local screen; expose the listener
only to trusted clients. kVNC logs the wake, acquisition, and release without
recording credentials.

Keep `scripts/kvnc-launch-macos.sh` as the LaunchAgent command when using this
option. Its one-time startup wake lets a restart recover after display sleep,
but does not keep the display lit. The connection setting is a separate,
post-authentication wake. The display must still have an active, unlocked GUI
session: this does not bypass the macOS login window, FileVault, Screen
Recording permission, or a physically disconnected monitor. Windows and Linux
use their respective platform-native paths documented above and in the Windows
guide.

## Starting through SSH

SSH can start `--source demo` directly. Do not use `sudo`. The following command assumes `source = "demo"` in the config. With the config daemon, the process remains after the SSH session closes.

```sh
ssh YOUR_USER@MAC_HOST '/Users/YOUR_USER/path/to/kvnc/bin/kvnc'
```

For `--source desktop`, an SSH shell commonly has no usable desktop-capture context and cannot answer the macOS Screen Recording consent prompt. Do not start it directly through SSH. Create and permit the LaunchAgent once from the logged-in Mac desktop, then use SSH only to control that existing user agent:

```sh
ssh YOUR_USER@MAC_HOST 'launchctl kickstart -k "gui/$(id -u)/com.kazunari-kamata.kvnc"'
ssh YOUR_USER@MAC_HOST 'launchctl print "gui/$(id -u)/com.kazunari-kamata.kvnc"'
ssh YOUR_USER@MAC_HOST 'tail -f /Users/YOUR_USER/path/to/kvnc/var/kvnc.log'
```

This requires that the same user has an active GUI login session. If no desktop session or monitor is available, `--source desktop` cannot capture a framebuffer; use `--source demo` for transport diagnostics. The LaunchAgent example above wakes a sleeping display before each start, then permits normal display sleep after startup.
