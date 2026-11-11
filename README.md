# kVNC

kVNC is a Rust command-line VNC server for macOS, Windows, and Linux. It implements RFB 3.8 Raw / Hextile encoding, the legacy clipboard messages (`ClientCutText` / `ServerCutText`), and the Extended Clipboard text format to synchronize plain text with the system clipboard in both directions.

Japanese documentation: [README.ja.md](README.ja.md).

Repository: [kazunari-kamata/kvnc](https://github.com/kazunari-kamata/kvnc)  
GitHub: [kazunari-kamata](https://github.com/kazunari-kamata) (Kazunari Kamata)

## Scope

kVNC captures a selected desktop through native OS APIs and publishes it at a configurable RFB endpoint. It is a CLI server: it has no local GUI. Keyboard and pointer injection are disabled by default; enable them explicitly with `--allow-input` only for trusted clients and after granting the required operating-system permission.

Except for the RA2ne credential exchange when `[auth]` is enabled, RFB traffic is unencrypted. kVNC binds to `127.0.0.1:5900` by default; retain that loopback-only setting unless transport security and network access controls are designed for the deployment.

## Install and run

For a Windows Git clone, PowerShell commands, desktop-session constraints, and
maintenance, see the [Windows guide](docs/windows.en.md). On Windows the native
binary is `bin\\kvnc.exe` and is invoked as `.\\bin\\kvnc.exe`.

Build a native standalone binary for the current platform:

```sh
cargo install --path . --root . --force --locked
bin/kvnc --version
bin/kvnc --source desktop --listen 127.0.0.1:5900 --debug --log-file kvnc.log
```

The installed `bin/kvnc` runs directly without Cargo. Grant desktop-capture permission when the operating system asks for it.

## Configuration-file startup

Argument-free startup reads the per-user configuration file, not a repository file. On macOS and Linux it is `${XDG_CONFIG_HOME:-~/.config}/kvnc/config.toml` (normally `~/.config/kvnc/config.toml`); on Windows it is `%APPDATA%\kvnc\config.toml`. Run `kvnc --init` to create the commented safe-default file, then review `listen` and `allow_input` in particular. This keeps listener addresses, log paths, and password verifiers out of Git.

```sh
bin/kvnc
```

Automatic user-configuration loading is intentionally limited to argument-free startup. A command such as `bin/kvnc --source desktop ...` uses CLI defaults and does **not** load `[auth]`; use `bin/kvnc` for the configured server, or pass `--config /path/to/kvnc.toml` explicitly.

To use any other file, including a project-local one, specify it explicitly:

```sh
bin/kvnc --config /path/to/kvnc.toml
```

Without a configuration file, argument-free startup retains the existing defaults: `127.0.0.1:5900`, desktop source, and input disabled.

### Settings and CLI options

| TOML key | Corresponding CLI | Values and defaults |
| --- | --- | --- |
| `listen` | `--listen` | Socket address; default `127.0.0.1:5900` |
| `source` | `--source` | `desktop` or `demo`; default `desktop` |
| `monitor` | `--monitor` | Zero-based index from `--list-monitors`; primary display when omitted |
| `allow_input` | `--allow-input` | Boolean; default `false` |
| `keyboard_layout` | `--keyboard-layout` | `auto`, `us`, or `jis` for macOS input; default `auto` |
| `scale` | `--scale` | `0.1` through `1.0`; default `1.0` |
| `fps` | `--fps` | `1` through `10`; default `2` |
| `debug` | `--debug` | Boolean; default `false` |
| `log_file` | `--log-file` | Relative or absolute log path |
| `log_rotation` | `--log-rotation` | `none` (default), `hourly`, `daily`, `weekly`, or `monthly` |
| `log_max_size_bytes` | `--log-max-size-bytes` | Rotate at this size; `0` (default) disables the size limit |
| `log_retention` | `--log-retention` | Timestamped archive files to retain; default `14`, `0` retains none |
| `daemon` | `--daemon` | Boolean; macOS/Linux only; requires `log_file` |
| `pid_file` | `--pid-file` | PID-file path; requires `daemon = true` |
| `[auth].username` | — | Optional login name; omit for password-only login |
| `[auth].password_hash` | `--generate-password-hash` (generation only) | Argon2id PHC verifier; never a plaintext password |

`--help`, `--version`, `--list-monitors`, `--print-config`, `--config <PATH>`, `--init`, `--update-config`, and `--update`/`--upgrade` are runtime-only options and have no TOML key. `--print-config` shows the effective settings and redacts the password verifier. `--update-config` creates a timestamped backup and adds missing safe defaults without replacing existing values. Unknown keys in a configuration file are errors.

For daemon operation, change `daemon` to `true`. With `log_file` and `pid_file` configured, argument-free startup works the same way.

```toml
daemon = true
pid_file = "kvnc.pid"
```

kVNC can rotate its own log without asking an external log manager to move an open file. Rotation occurs when either the configured period or the size limit is reached; the earlier condition wins. The active path remains `kvnc.log`; archives are named `kvnc.log.<Unix-timestamp>`. `monthly` is a rolling 30-day interval, not a calendar-month boundary. For example:

```toml
log_file = "/absolute/path/kvnc.log"
log_rotation = "daily"
log_max_size_bytes = 104857600 # 100 MiB; use 0 to disable size rotation
log_retention = 14
```

For an RFB and clipboard-only verification that does not need desktop-capture permission:

```sh
bin/kvnc --source demo --listen 127.0.0.1:5900
```

Connect an RFB 3.8-compatible VNC client to `127.0.0.1:5900`.

`--debug` records the RFB handshake and each client message without writing clipboard contents. `--log-file` appends the same output to a file, which is useful when diagnosing a client disconnect. With an Extended Clipboard-capable client, kVNC negotiates capabilities and synchronizes UTF-8 text bidirectionally using Notify / Request / Provide with zlib compression; other clients retain legacy `ClientCutText` / `ServerCutText` interoperability.

v0.2.0 selects Hextile when a client advertises it and otherwise uses Raw. Hextile is lossless: it preserves source pixels while reducing transfer volume for flat desktop areas without reducing `scale`. kVNC sends an initial full frame, then sends only the smallest changed rectangle for incremental requests. It keeps at most one framebuffer update queued and captures no more often than `--fps`; keyboard/pointer events are handled separately, so input events do not wait behind an ongoing framebuffer transfer. Use `--fps 5` normally and `--fps 1` when reducing bandwidth is more important than refresh rate.

Use `--scale 0.5` only when Hextile is still too slow. A 2880x1800 Retina capture becomes 1440x900, reducing worst-case transfer volume to roughly one quarter. `scale = 1.0` preserves the native capture resolution and gives the sharpest text. Input coordinates are converted automatically for the scaled framebuffer.

```sh
bin/kvnc --source desktop --monitor 0 --allow-input --scale 0.5 \
  --listen 127.0.0.1:5900
```

By default, kVNC uses RFB SecurityType None: no username or password is required. If a client displays credential fields anyway, leave both fields empty. Restrict access to loopback or a trusted encrypted private network.

To require a username and password, generate an Argon2id verifier interactively (the password is not echoed or saved), then add the printed value to `[auth]` in the per-user configuration file.

```sh
bin/kvnc --generate-password-hash
```

```toml
[auth]
username = "operator"
password_hash = "$argon2id$..."
```

For password-only login, omit `username`; kVNC requests the RA2ne password-only credential subtype.

With `[auth]`, kVNC advertises RFB RA2ne authentication. It encrypts the username/password exchange with an ephemeral RSA/AES handshake and verifies the supplied password against the stored Argon2id verifier. RA2ne does **not** encrypt subsequent framebuffer, input, or clipboard traffic; continue using a trusted encrypted private network or SSH tunnel. A client may ask to approve the server key because it is generated at startup.

List and select a monitor. Run one kVNC process per monitor when separate VNC views are desired:

```sh
bin/kvnc --list-monitors
bin/kvnc --source desktop --monitor 1 --allow-input --listen 127.0.0.1:5901
```

On macOS, `--allow-input` also requires Accessibility permission for the launching application.

On macOS, printable RFB keysyms are injected as physical key codes. This retains JIS keyboard and IME key bindings; it is not a UTF-8 conversion. `keyboard_layout = "jis"` (or `--keyboard-layout jis`) maps JIS `@` (voiced mark), `[` (semi-voiced mark), `^` (he), `¥` (prolonged sound mark), `ろ`, `け`, and `む` to their distinct macOS JIS key codes for direct Kana entry with the Japanese Kana input source. Use the default `auto` for ordinary layouts. Debug logs retain keysyms only, never typed text.

## Documentation

- [Installation](INSTALL.md)
- [Windows guide](docs/windows.en.md)
- [Protocol and security model](docs/protocol.en.md)
- [Platform compatibility](docs/compatibility.en.md)
- [Background operation](docs/daemon.en.md)
- [Verification guide](docs/verification.en.md)
- [Security policy](docs/security.en.md)
- [Contributing](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [Third-party license notices](THIRD_PARTY_LICENSES.html)

## License

kVNC is licensed under the [MIT License](LICENSE). Each binary release archive
includes this file and `THIRD_PARTY_LICENSES.html`, which identifies the locked
Rust dependencies and preserves their full license texts. Regenerate the notice
after changing `Cargo.lock` with `scripts/generate-third-party-licenses.sh`
(requires `cargo-about` 0.9.3).
