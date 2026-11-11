# Installation

Japanese: [INSTALL.ja.md](INSTALL.ja.md).

## Requirements

- Rust stable toolchain
- A desktop session for `--source desktop`
- Desktop-capture permission where required by the OS

On Linux, install the development packages required to compile desktop capture support:

```sh
sudo apt-get install libdbus-1-dev libpipewire-0.3-dev libwayland-dev libx11-dev libxrandr-dev
```

## Build a standalone executable

```sh
cargo install --path . --root . --force --locked
bin/kvnc --version
```

`bin/kvnc` is a native executable for the current platform. It does not require Cargo at runtime.

For Windows, use PowerShell and invoke `.\bin\kvnc.exe`; the full clone,
configuration, and update procedure is in the [Windows guide](docs/windows.en.md).

## Run

```sh
bin/kvnc --source desktop --listen 127.0.0.1:5900 --debug --log-file kvnc.log
```

Use `--source demo` to exercise RFB and clipboard handling without desktop capture. See [Platform compatibility](docs/compatibility.en.md) before deploying outside localhost.

### macOS permission and `sudo`

Do **not** run kVNC with `sudo`. The process that captures the desktop must be the signed-in desktop user, and `sudo` gives the root process a separate privacy-permission identity. In **System Settings → Privacy & Security → Screen Recording**, enable the application that launches kVNC (for example Terminal or the IDE), then quit and reopen that application before retrying. A direct standalone binary can also appear as `kvnc` in this list.

To allow keyboard and pointer control, add `--allow-input`. On macOS, also allow the launching application in **System Settings → Privacy & Security → Accessibility**, then restart it. `--debug --log-file kvnc.log` records permission and RFB connection failures without storing clipboard text.

### A client on another trusted host

The default `127.0.0.1:5900` listener accepts only clients on the same machine. When the client is on another host, bind kVNC to the specific address of a trusted private network interface, not to every interface. Replace `SERVER_PRIVATE_IP` with the server's trusted-overlay address:

```sh
bin/kvnc --source demo --listen SERVER_PRIVATE_IP:5900 --debug --log-file kvnc.log
```

First confirm that the demo source connects, then replace `demo` with `desktop`. Password-free RFB has no encryption or authentication. Configuring `[auth]` enables RA2ne authentication, which encrypts only the credential exchange; it does not encrypt the subsequent RFB session. Never expose this listener to an untrusted network.

See [Background operation](docs/daemon.en.md) for `--daemon` and macOS launchd user-agent operation.
