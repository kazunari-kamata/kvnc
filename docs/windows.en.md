# Windows guide

Japanese: [windows.ja.md](windows.ja.md).

## Clone, build, and verify

Install Git, the stable Rust **MSVC** toolchain, and the Visual Studio Build
Tools with the Windows SDK. Open PowerShell as the signed-in desktop user, then
clone kVNC and run the installer:

```powershell
git clone https://github.com/kazunari-kamata/kvnc.git
Set-Location kvnc
.\scripts\install-windows.ps1
```

The installer checks for the Rust MSVC host toolchain, builds with
`cargo build --release --locked`, copies the result to `bin\kvnc.exe`, verifies
its version, and creates `%APPDATA%\kvnc\config.toml` only when it does not
already exist. It never starts a server, overwrites an existing configuration,
changes the firewall, elevates privileges, or terminates a running kVNC process.
Stop a running instance before rebuilding so Windows can replace its executable.

From Command Prompt, use the equivalent wrapper:

```cmd
scripts\install-windows.cmd
```

`bin\kvnc.exe` is a native standalone executable. Cargo is needed to build or
update it, but not to run it afterward. Building below `target\` keeps the
source checkout clean for later `--update` use. Do not use `sudo`-like elevation or a
Windows service for an interactive desktop capture: run it in the same signed-in
user session whose desktop, clipboard, keyboard, and pointer are to be shared.

The initial configuration deliberately leaves input disabled. On a new install
for a trusted listener and trusted clients, opt in explicitly:

```powershell
.\scripts\install-windows.ps1 -AllowInput
```

`-AllowInput` applies only while the installer creates a new configuration; an
existing configuration is always preserved. Use `-Config C:\path\to\kvnc.toml`
to create or preserve a different configuration path, or `-SkipConfig` to only
build and deploy the executable.

If automation is unsuitable, the installer performs the following manual
equivalent:

```powershell
cargo build --release --locked
New-Item -ItemType Directory -Force -Path bin | Out-Null
Copy-Item -Force .\target\release\kvnc.exe .\bin\kvnc.exe
.\bin\kvnc.exe --version
```

## Initial configuration

The installer normally creates the commented per-user configuration safely. It
is not created inside the Git checkout and is therefore not pushed by Git. To
create it manually or after `-SkipConfig`:

```powershell
.\bin\kvnc.exe --init
.\bin\kvnc.exe --print-config
notepad "$env:APPDATA\kvnc\config.toml"
```

The normal path is `%APPDATA%\kvnc\config.toml`. `--init` refuses to replace
an existing file. Use `--init --force` only after reviewing it. To create a
different file, use `--config C:\path\to\kvnc.toml --init`.

The generated configuration starts safely with loopback-only listening and
input disabled. It uses `scale = 1.0` to preserve the captured desktop's native
resolution and keep text sharp. Review `listen` and `allow_input` before
starting the server. Its log file defaults to `%APPDATA%\kvnc\kvnc.log`.

`allow_input = false` means that mouse and keyboard events from the VNC client
are intentionally ignored. For a trusted listener and trusted clients, either
change it to `true` in the configuration, or create the initial configuration
with the explicit opt-in below:

```powershell
.\bin\kvnc.exe --init --allow-input
```

Existing configurations created by older kVNC versions may contain
`scale = 0.5`; change it to `1.0` and restart kVNC for full-resolution capture.
Also disable any client-side "scale to fit" or viewport scaling when inspecting
sharpness.

## Start and test

Run the configured desktop server without arguments:

```powershell
.\bin\kvnc.exe
```

For an RFB/clipboard-only test that does not require desktop capture, run:

```powershell
.\bin\kvnc.exe --source demo --listen 127.0.0.1:5900 --debug --log-file .\kvnc.log
```

List monitors before selecting a desktop monitor:

```powershell
.\bin\kvnc.exe --list-monitors
.\bin\kvnc.exe --source desktop --monitor 0 --allow-input --listen 127.0.0.1:5900
```

The Windows lock screen and UAC secure desktop are separate protected desktops;
do not expect ordinary user-session capture or input injection there. For a
client on a trusted private network, bind to one specific private-interface
address and allow only that trusted network in Windows Defender Firewall. Do
not expose unencrypted RFB to the public Internet.

## Start at sign-in and handle sleep

To request a short display wake after a successful desktop VNC connection, add
the following to `%APPDATA%\kvnc\config.toml`:

```toml
wake_display_on_connect = true
```

kVNC uses `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED)` on a
dedicated thread for five seconds, then clears that request. This is the
Windows counterpart to a short user-activity wake: it does not permanently
change the power plan, inject mouse or keyboard input, unlock Windows, or make
the UAC secure desktop capturable. It is invoked only after the RFB handshake
(and RA2ne authentication, when configured) has completed. Confirm it with
the `requested Windows display wake` log line.

To restart kVNC at sign-in, use **Task Scheduler** to create a task with these
properties:

1. On **General**, select the desktop user and choose **Run only when user is logged on**. Do not select “Run whether user is logged on or not,” and do not use a Windows service.
2. Add an **At log on** trigger for that user.
3. Add a **Start a program** action whose program is the absolute path to `kvnc.exe`. It reads that user's `%APPDATA%\kvnc\config.toml` without command-line arguments.
4. In **Settings**, optionally enable restart-on-failure. Keep the task in the interactive user session; it must not be moved to Session 0.

After creating the task, sign out and sign in, then verify the listener from
the same user session:

```powershell
Get-Process kvnc
Get-NetTCPConnection -LocalPort 5900 -State Listen
Get-Content "$env:APPDATA\kvnc\kvnc.log" -Tail 50
```

Sleep, hibernation, lock-screen transitions, and the display power timeout can
make an interactive desktop unavailable for capture or input. Choose the
appropriate **Settings → System → Power & battery → Screen and sleep** policy
for the host's trusted remote-access use, and test `wake_display_on_connect`
on both AC and battery power. Do not weaken the lock-screen or power policy
merely to run kVNC; a locked or secure desktop remains intentionally outside
this user-session server's scope.

## Quick local start, stop, and restart

For development or one-off local operation, the repository includes a
PID-file-based control helper with a Command Prompt wrapper. It controls only
the process it started and does not stop an unrelated Task Scheduler instance:

```cmd
scripts\kvncctl.cmd status
scripts\kvncctl.cmd stop
scripts\kvncctl.cmd start
scripts\kvncctl.cmd restart
```

The `.cmd` wrapper safely delegates to the reviewed PowerShell implementation;
use either this helper or the Task Scheduler task for one listener, not both.
See [Instance control and development workflow](control.en.md) for listener
diagnostics, separate development ports, and the PowerShell policy command.

## Maintain a clone

`--update-config` preserves existing values and comments, adds only missing
top-level defaults, and creates a timestamped backup beside the configuration:

```powershell
.\bin\kvnc.exe --update-config
```

`--update` (or `--upgrade`) operates on a clean Git or JJ source checkout. A
Git checkout must have a current upstream branch and is fast-forwarded with
`git pull --ff-only`. A JJ checkout must have an empty working-copy change;
kVNC fetches and starts a new empty change from `main@origin`, refusing to
hide any non-empty change. Use a clone dedicated to running kVNC, not a feature
or pull-request checkout. Windows cannot replace the running `kvnc.exe`, so it
builds `target\release\kvnc.exe` and prints the required replacement command.
After the command exits, run:

```powershell
.\bin\kvnc.exe --update
Copy-Item -Force .\target\release\kvnc.exe .\bin\kvnc.exe
.\bin\kvnc.exe --version
```

For a downloaded GitHub Release artifact rather than a clone, download and
replace the artifact manually; `--update` intentionally refuses to modify it.
