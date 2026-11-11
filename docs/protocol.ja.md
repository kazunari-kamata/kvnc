# プロトコルと security model

English: [protocol.en.md](protocol.en.md)

kVNC は意図的に小さい RFB 3.8 server surface を実装します。

- RFB protocol version `003.008`、RFB `003.003` 互換性
- 既定の Security type `None`、または `[auth]` 設定時の RA2ne（type `6`）
- 32-bit little-endian true-colour framebuffer
- client が提示した場合の Hextile framebuffer encoding、fallback としての Raw encoding
- UTF-8 plain text 用の `ClientCutText` / `ServerCutText`、および Extended Clipboard text format（Caps / Notify / Request / Provide）
- `--allow-input` 指定時の optional `KeyEvent` / `PointerEvent` 注入

server は framebuffer update request を受け取り、Hextile を提示した client には Hextile rectangle、それ以外には Raw rectangle を送信します。最初の update は全画面で、その後の incremental update は変更された最小 rectangle だけを送信します。client がこの server surface を使い続けられるよう、`EnableContinuousUpdates`、`ClientFence`、`SetDesktopSize` の extension message は受け付けますが適用はしません。classic VNC password authentication（type `2`）、TLS、clipboard file、desktop resize、Zlib/Tight 等の追加 encoding は実装しません。

## Security boundary

RFB SecurityType None には authentication も encryption もありません。このため kVNC は既定で `127.0.0.1:5900` に bind します。VPN または SSH tunnel など、別管理の authenticated / encrypted transport で access を制限しない限り、`--listen` を LAN や Internet から到達可能な address へ変更しないでください。

`[auth]` に Argon2id PHC password verifier と optional な username を設定した場合、kVNC は RA2ne を使用します。RA2ne は ephemeral RSA/AES exchange で credential を通信中に暗号化し、kVNC は plaintext credential を保存しません。ただし authentication 後の RFB session は暗号化されないため、VPN、SSH tunnel、TLS transport の代替ではありません。

clipboard text には sensitive material が含まれる場合があります。kVNC は永続化しませんが、接続済み client は clipboard update を受信できます。trusted client のみを使用してください。
