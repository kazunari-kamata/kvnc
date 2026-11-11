# Protocol and security model

Japanese: [protocol.ja.md](protocol.ja.md).

kVNC implements a deliberately small RFB 3.8 server surface:

- RFB protocol version `003.008`, with RFB `003.003` compatibility
- Security type `None` by default, or RA2ne (type `6`) when `[auth]` is configured
- 32-bit little-endian true-colour framebuffer
- Hextile framebuffer encoding when offered by the client, with Raw fallback
- `ClientCutText` / `ServerCutText` and Extended Clipboard text (Caps / Notify / Request / Provide) for UTF-8 plain text
- Optional `KeyEvent` and `PointerEvent` injection when `--allow-input` is set

The server accepts framebuffer update requests and publishes Hextile rectangles when the client offers Hextile, otherwise Raw rectangles. The first update is full-screen; later incremental updates contain only the smallest changed rectangle. It accepts (but does not apply) `EnableContinuousUpdates`, `ClientFence`, and `SetDesktopSize` client extension messages so a client can continue using this server surface. It does not implement classic VNC password authentication (type `2`), TLS, clipboard files, desktop resizing, or additional encodings such as Zlib/Tight.

## Security boundary

RFB SecurityType None provides no authentication or encryption. kVNC therefore defaults to `127.0.0.1:5900`. Do not change `--listen` to a LAN or Internet-reachable address unless a separately managed authenticated and encrypted transport, such as a VPN or SSH tunnel, restricts access.

When `[auth]` supplies an Argon2id PHC password verifier and an optional username, kVNC uses RA2ne. RA2ne encrypts credentials in transit using an ephemeral RSA/AES exchange; the verifier is one-way and kVNC never stores plaintext credentials. RA2ne does not encrypt the RFB session after authentication, so it is not a replacement for a VPN, SSH tunnel, or TLS transport.

Clipboard text may contain sensitive material. kVNC does not persist it, but connected clients can receive clipboard updates. Use only trusted clients.
