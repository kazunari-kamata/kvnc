# Security policy

Japanese: [security.ja.md](security.ja.md).

Report a vulnerability privately to the repository owner. Do not publish credentials, clipboard contents, capture data, or a working exploit in a public issue.

The Security workflow scans tracked files for common committed secret markers, runs CodeQL for Rust, and runs `cargo audit`. These checks complement, but do not replace, review of protocol changes and deployment controls.

## Remote control and passwords

`--allow-input` gives a connected client control of the local keyboard and pointer. It is disabled by default. Enable it only on a loopback listener or a trusted private network with separate access controls.

When `[auth]` is absent, kVNC is deliberately password-free. When it is present, kVNC uses RA2ne (SecurityType 6): it encrypts the username/password exchange with an ephemeral RSA/AES handshake, and stores only an Argon2id PHC password verifier. Generate it with `bin/kvnc --generate-password-hash`; do not write a plaintext password to the per-user configuration file.

Classic VNC password authentication (SecurityType 2) remains unsupported: it uses a reversible, DES-derived key and truncates passwords to eight bytes. RA2ne protects credentials but does not encrypt the RFB session after login. Use a trusted encrypted overlay network or SSH tunnel and do not expose an input-enabled listener to an untrusted network.

The RA2ne RSA key pair is generated once at kVNC startup and shared by every connection in that process. Debug logs record only the SHA-256 fingerprint of its public wire value at generation and connection phases; a changed fingerprint indicates a new kVNC process, not per-connection rekeying.
