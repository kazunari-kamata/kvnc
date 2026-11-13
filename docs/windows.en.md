# Windows guide

Japanese: [windows.ja.md](windows.ja.md).

## Clone, build, and verify

Install Git, the stable Rust **MSVC** toolchain, and the Visual Studio Build
Tools with the Windows SDK. Open PowerShell as the signed-in desktop user, then
clone and build kVNC:

```powershell
git clone https://github.com/kazunari-kamata/kvnc.git
Set-Location kvnc
cargo install --path . --root . --force --locked
.\bin\kvnc.exe --version
.\bin\kvnc.exe --help
```

`bin\kvnc.exe` is a native standalone executable. Cargo is needed to build or
update it, but not to run it afterward. Do not use `sudo`-like elevation or a
Windows service for an interactive desktop capture: run it in the same signed-in
user session whose desktop, clipboard, keyboard, and pointer are to be shared.

## Initial configuration

Create the commented per-user configuration safely. It is not created inside
the Git checkout and is therefore not pushed by Git:

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

There is no Windows equivalent of the macOS `caffeinate` wrapper in kVNC, and
kVNC does not alter the computer's power policy. To restart it at sign-in, use
**Task Scheduler** to create a task with these properties:

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
for the host's trusted remote-access use, and test it on both AC and battery
power. Do not weaken the lock-screen or power policy merely to run kVNC; a
locked or secure desktop remains intentionally outside this user-session
server's scope.

## Maintain a clone

`--update-config` preserves existing values and comments, adds only missing
top-level defaults, and creates a timestamped backup beside the configuration:

```powershell
.\bin\kvnc.exe --update-config
```

`--update` (or `--upgrade`) only operates on a clean Git clone. It performs
`git pull --ff-only`. Windows cannot replace the running `kvnc.exe`, so it then
prints the required rebuild command. After the command exits, run:

```powershell
.\bin\kvnc.exe --update
cargo install --path . --root . --force --locked
.\bin\kvnc.exe --version
```

For a downloaded GitHub Release artifact rather than a clone, download and
replace the artifact manually; `--update` intentionally refuses to modify it.
