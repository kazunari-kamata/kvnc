# Platform compatibility

Japanese: [compatibility.ja.md](compatibility.ja.md).

| Platform | Desktop capture | Clipboard | CI verification |
| --- | --- | --- | --- |
| macOS arm64 | Supported with Screen Recording permission | Supported | Native build and tests on `macos-14` |
| macOS x86_64 | Supported with Screen Recording permission | Supported | Native build and tests on `macos-15-intel` |
| Windows x86_64 | Supported in an interactive desktop session | Supported | Native build and tests on `windows-latest` |
| Linux x86_64 / X11 | Supported in an interactive desktop session | Supported | Native build and tests on `ubuntu-latest` |
| Linux / Wayland | Environment and portal dependent | Environment dependent | Native build and tests on Ubuntu; smoke test required |

`--source demo` does not capture the desktop and is the portable way to verify RFB protocol behaviour. A CI build cannot grant screen-capture permission or create every real desktop session; run a smoke test on the deployment OS before relying on `--source desktop`.

## macOS setup

Run kVNC as the logged-in desktop user, without `sudo`. Allow the launcher application in **System Settings → Privacy & Security → Screen Recording**, then fully restart that launcher. Use `--source demo --debug --log-file kvnc.log` to first confirm the RFB connection independently of Screen Recording permission.

## Windows setup

Run kVNC in the signed-in interactive user session, not as a Windows service.
The lock screen and UAC secure desktop are protected desktops and are outside
ordinary user-session capture and input injection. See the [Windows guide](windows.en.md)
for the Git clone, `--init`, `--update-config`, and `--update` procedures.
