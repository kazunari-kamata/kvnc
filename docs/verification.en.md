# Verification guide

Japanese: [verification.ja.md](verification.ja.md).

Use this procedure for protocol, listener, clipboard, desktop-capture, and background-operation changes. It deliberately starts with `--source demo` so network and RFB failures are not confused with operating-system capture permissions.

## 1. Build and run a local protocol check

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
mkdir -p var
bin/kvnc --source demo --listen 127.0.0.1:5900 \
  --debug --log-file "$PWD/var/kvnc.log"
```

Connect a compatible RFB client on the same machine. The log must include `client accepted`, `client connected`, and a `FramebufferUpdateRequest`; it must not contain `ERROR`.

The default server uses SecurityType None. Leave client username and password fields empty when they are shown by a generic client UI. To verify password login, generate a throwaway verifier with `bin/kvnc --generate-password-hash`, add it with a test username in `[auth]`, restart kVNC, and confirm that the client selects RA2ne and accepts only the configured credentials. Remove the test `[auth]` section afterwards if password-free operation is intended.

For handshake changes, verify both RFB `003.008` and `003.003`. A 3.3 client must log `handshake: RFB 3.3 security_type=None`; a modern client must log `handshake: RFB 3.7+ security_type=None`.

## 2. Verify a trusted remote listener

Use the specific private-network address assigned to the kVNC host, rather than `0.0.0.0`, for a client on another machine:

```sh
bin/kvnc --source demo --listen SERVER_PRIVATE_IP:5900 \
  --debug --log-file "$PWD/var/kvnc.log"
lsof -nP -iTCP:5900 -sTCP:LISTEN
```

From the client host, confirm TCP reachability before opening the RFB client:

```sh
nc -vz -w 3 SERVER_PRIVATE_IP 5900
```

Then connect an RFB client whose exact target is `tcp://SERVER_PRIVATE_IP:5900`. While connecting, run `tail -f var/kvnc.log` on the server.

| Server log result | Meaning and next action |
| --- | --- |
| No `client accepted` | Check the client target, exact bind address, private-network ACL, and host firewall. |
| `client accepted` then `client connected` | TCP and RFB negotiation succeeded. Check the framebuffer request next. |
| `client disconnected` | Preserve the message type and reason from the log for an interoperability report. |
| `FramebufferUpdateRequest` and `framebuffer update sent` | The client received an RFB framebuffer response. |

Password-free RFB has no encryption or authentication. RA2ne protects only the configured credential exchange, not the subsequent session. Do not use this procedure on an untrusted network.

## 3. Verify clipboard

With the client connected to the demo source, use disposable plain text:

1. Copy text on the client and paste it into a local editor on the kVNC host.
2. Copy different text in a local editor on the kVNC host and paste it into the client.
3. With an Extended Clipboard-capable client, confirm `extended clipboard capabilities received` and `extended clipboard received` in the log. A legacy client logs clipboard byte counts instead.
4. Confirm that `var/kvnc.log` never contains clipboard contents.

## 4. Verify desktop capture

Only after the demo test passes, run `--source desktop` from an interactive desktop login. On macOS, allow the launcher in **System Settings → Privacy & Security → Screen Recording**, restart the launcher, and do not use `sudo`. If the server reports `no monitor is available`, stop protocol investigation: the host has no captureable desktop context, display, or permission.

If the image is uniformly black, first unlock the Mac and wake its displays. While `system_profiler SPDisplaysDataType` reports `Display Asleep: Yes` for a display, macOS can return a black capture image. To prevent sleep during continuous operation, run `caffeinate -dimsu` in a trusted local session and stop it with `Ctrl-C` when finished.

## 5. Verify background operation

On macOS or Linux, confirm `--daemon` leaves a listener, writes the requested PID, and writes logs. Use the documented LaunchAgent route for macOS desktop capture; do not add `--daemon` to the LaunchAgent arguments. See [Background operation](daemon.en.md).

## 6. Verify remote input and multiple monitors

Use `--list-monitors` to obtain stable zero-based indices. Start a separate kVNC process and listener port for each monitor; this provides one independent RFB framebuffer per display.

```sh
bin/kvnc --list-monitors
bin/kvnc --source desktop --monitor 0 --allow-input --listen 127.0.0.1:5900
bin/kvnc --source desktop --monitor 1 --allow-input --listen 127.0.0.1:5901
```

Verify pointer movement, left/middle/right buttons, wheel scrolling, text keys, modifier keys, navigation keys, and F1–F12. On macOS, grant the launcher Accessibility permission before this test. Do not enable input on an untrusted network.

On a Retina primary display, kVNC converts captured framebuffer pixels to macOS logical input points using the selected monitor's scale factor. Confirm that a click near each corner lands at the corresponding local-screen position.

### Drag-selection diagnosis

Start with a new debug log, then make exactly one slow left-button drag across disposable text. kVNC logs time to millisecond precision. A valid drag contains an initial `mask=0x01`, one or more following `mask=0x01` pointer events with changing coordinates, then `mask=0x00`. Each `client message: PointerEvent` must be immediately followed by its matching `pointer event injected` line with the same coordinates. If the held-button events are absent, the client or bridge did not send a drag; if they are present and injection is immediate but macOS selects the wrong text, capture the matching log excerpt for kVNC input investigation.

```sh
rg -n 'PointerEvent|pointer event injected' var/kvnc.log | tail -n 80
```
