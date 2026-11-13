# Background operation

Japanese: [daemon.ja.md](daemon.ja.md).

## Foreground startup from Terminal

For the first `--source desktop` startup and permission grant, use Terminal.app or iTerm2 opened in the signed-in Mac GUI session. Do not use an SSH shell or a terminal on the remote-client host. With `~/.config/kvnc/config.toml` configured, no arguments are required from any working directory:

```sh
/Users/YOUR_USER/path/to/kvnc/bin/kvnc
```

Stop the foreground process with `Control-C` and run the same command to start it again. Grant macOS Screen Recording and Accessibility permission to this local GUI launch context. Do not use `sudo`.

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

kVNC owns rotation of `log_file`: it archives the previous file as `kvnc.log.<Unix-timestamp>` and creates a fresh active file at the original path. `log_rotation` accepts `none`, `hourly`, `daily`, `weekly`, and `monthly`; `monthly` is a rolling 30-day interval. A nonzero `log_max_size_bytes` triggers an earlier rotation. `log_retention` controls the number of archives kept. This works for foreground, daemon, and LaunchAgent operation without reopening the process.

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

`systemd-inhibit` asks logind not to perform idle or sleep actions while kVNC is running; it does not change the desktop environment's display blanking, screen-lock, or Wayland portal policy. Configure those separately if a sleeping display cannot be captured. On X11, desktop capture must see the same `DISPLAY` and X authority as the graphical session. On Wayland, the compositor and screen-capture portal decide whether a user service can capture; run `--source demo` first and then smoke-test the actual desktop session. If the distribution or desktop environment does not provide `graphical-session.target`, use its graphical-session startup facility instead of enabling linger or moving kVNC into a system service.

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
    <string>/usr/bin/caffeinate</string>
    <string>-d</string>
    <string>-i</string>
    <string>-u</string>
    <string>/Users/YOUR_USER/path/to/kvnc/bin/kvnc</string>
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

The `caffeinate` wrapper makes a short user-activity assertion on startup and prevents display and idle system sleep while kVNC is running. Without it, a sleeping display is not an active monitor to the capture API, and kVNC cannot start desktop capture. The user configuration at `~/.config/kvnc/config.toml` is loaded automatically; `WorkingDirectory` does not select configuration. Do not enable `daemon` in that config when using a LaunchAgent: launchd owns restart and process lifecycle. Permit the launching context in **System Settings → Privacy & Security → Screen Recording** and, when remote input is enabled, **Accessibility**, then restart the agent after changing either permission. If input permission is unavailable, kVNC stays available in view-only mode and records the reason in its log. To remove the agent, run `launchctl bootout "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist"`.

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

This requires that the same user has an active GUI login session. If no desktop session or monitor is available, or all displays are asleep, `--source desktop` cannot capture a framebuffer; use `--source demo` for transport diagnostics. The LaunchAgent example above prevents display sleep after startup.
