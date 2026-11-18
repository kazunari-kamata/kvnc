# Instance control and development workflow

Japanese: [control.ja.md](control.ja.md).

This guide distinguishes the three different uses of `caffeinate`, and adds
safe start, stop, restart, status, and log commands for one kVNC instance.
Run all commands as the signed-in desktop user; never use `sudo`.

## `caffeinate`: what to stop

| Use | Lifetime | Correct stop method |
| --- | --- | --- |
| LaunchAgent wrapper: `scripts/kvnc-launch-macos.sh` (one `caffeinate -u -t 5`, then `caffeinate -i`) | Continues while the LaunchAgent job is loaded; the display assertion lasts only five seconds at startup | Unload the LaunchAgent; do **not** kill `caffeinate` directly |
| Manual Terminal command: `caffeinate -dimsu` | Continues until its Terminal process exits | Press `Control-C` in that Terminal, or signal the PID you started and verified |
| Connection wake: complete `caffeinate -u -t 5`, then client-scoped `caffeinate -d` | From an authenticated VNC connection until that client disconnects | Disconnect the VNC client; kVNC releases the assertion |

The LaunchAgent example uses `KeepAlive = true`. Therefore killing its
`caffeinate` child merely causes `launchd` to start it again. Stop the job,
which stops both the wrapper and kVNC and prevents that restart:

```sh
launchctl bootout "gui/$(id -u)/com.kazunari-kamata.kvnc"
```

The plist remains in `~/Library/LaunchAgents/`; this command does not delete
it. Do not use `pkill caffeinate` or `killall caffeinate`, because other apps
may legitimately hold their own power assertions.

## `kvncctl` quick control

The repository provides these local helpers:

- macOS/Linux: `scripts/kvncctl`
- Windows Command Prompt: `scripts\kvncctl.cmd`, a thin launcher for
  `scripts\kvncctl.ps1`
- Windows PowerShell: `scripts\kvncctl.ps1` for direct PowerShell use

They intentionally control only an instance they can identify safely:

- On macOS LaunchAgent mode controls only
  `gui/<uid>/com.kazunari-kamata.kvnc`.
- On macOS/Linux daemon mode controls only the PID in its configured PID file,
  and verifies that PID still belongs to the expected `bin/kvnc` path.
- On Windows the PowerShell helper controls only the PID file that it created
  and verifies the executable path through `Win32_Process`.

An unrelated foreground kVNC, an unknown PID, or another application's
`caffeinate` is never killed. `status` also shows listeners on port 5900 (or
the supplied `--port`) to help find an existing server.

### macOS LaunchAgent

After creating the standard plist from the [background-operation guide](daemon.en.md), use:

```sh
scripts/kvncctl status
scripts/kvncctl stop
scripts/kvncctl start
scripts/kvncctl restart
```

With no mode selected, macOS selects LaunchAgent mode when
`~/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist` exists. `stop` uses
`launchctl bootout`; `start` bootstraps the plist when needed; `restart` is the
safe operation after a binary, configuration, or permission change. Use
`scripts/kvncctl logs` when `KVNC_LOG_FILE` names the active kVNC log, for
example:

```sh
KVNC_LOG_FILE=/Users/YOUR_USER/path/to/kvnc/var/kvnc.log scripts/kvncctl logs
```

To inspect the agent without the helper:

```sh
launchctl print "gui/$(id -u)/com.kazunari-kamata.kvnc"
```

### macOS/Linux daemon

For a daemon managed from a configuration file, `log_file` is required. The
helper adds `--daemon` and `--pid-file`, so `daemon = true` need not be stored
in the file. Set absolute log and PID paths for an operational instance:

```toml
log_file = "/Users/YOUR_USER/path/to/kvnc/var/kvnc.log"
# `daemon = true` is optional when scripts/kvncctl starts it.
```

```sh
scripts/kvncctl --daemon \
  --config "$HOME/.config/kvnc/config.toml" \
  --pid-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.pid \
  --log-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.log \
  status
scripts/kvncctl --daemon \
  --config "$HOME/.config/kvnc/config.toml" \
  --pid-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.pid \
  --log-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.log \
  restart
```

On Linux, prefer the documented graphical-session `systemd --user` unit for
long-lived desktop capture. Its lifecycle is owned by systemd, so use
`systemctl --user restart kvnc.service` rather than `kvncctl` for that unit.

### Windows Command Prompt and PowerShell

Run the Command Prompt wrapper from the signed-in interactive desktop user
session:

```cmd
scripts\kvncctl.cmd status
scripts\kvncctl.cmd stop
scripts\kvncctl.cmd start
scripts\kvncctl.cmd restart
scripts\kvncctl.cmd logs
```

The `.cmd` wrapper starts PowerShell with `-NoProfile` and forwards all
arguments to the reviewed `.ps1` implementation using `-ExecutionPolicy
Bypass`; it does not bypass kVNC authentication or grant administrator rights.
Use the `.ps1` directly when PowerShell parameters or interactive debugging
are preferred. The helper starts `bin\kvnc.exe` directly with the user's configuration and
records its PID below `var\`. It is for foreground-development convenience,
not a replacement for the documented Task Scheduler logon task. Do not run
both against the same listener. For direct PowerShell invocation, review the
file and use:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\kvncctl.ps1 status
```

## Existing server and development work

Before starting a development server, identify the server that already owns
the listener:

```sh
scripts/kvncctl status
```

If it is the standard LaunchAgent, use `scripts/kvncctl stop`, build or update
the binary, then use `scripts/kvncctl restart` or `start`. If it is a known
daemon started through the helper, pass the same `--pid-file` and use
`--daemon stop`. If `status` shows a listener but says it is unmanaged, inspect
the displayed PID and command before acting. Typical safe cases are:

- A Terminal foreground server: return to that Terminal and press `Control-C`.
- A known LaunchAgent: use `launchctl bootout` above.
- A separately managed `systemd --user` service: use `systemctl --user stop kvnc.service`.

Do not launch a second server on the same address and port. To keep a
production server running while exercising a development build, create a
private, ignored `config.dev.toml` with a different listener, PID file, and
log file. `config.*.toml` is ignored by Git:

```toml
# config.dev.toml
listen = "127.0.0.1:5901"
source = "demo"
allow_input = false
debug = true
log_file = "var/kvnc-dev.log"
```

Run it in the foreground while developing:

```sh
bin/kvnc --config config.dev.toml
```

Or manage that separate daemon explicitly:

```sh
scripts/kvncctl --daemon --config config.dev.toml \
  --pid-file var/kvnc-dev.pid --log-file var/kvnc-dev.log --port 5901 start
```

Use `source = "demo"` first. If testing desktop capture in parallel, keep
remote input disabled unless the test is intentional and no other remote
operator is controlling the same desktop.
