# Security policy

English: [security.en.md](security.en.md)

脆弱性は repository owner へ private に報告してください。credential、clipboard content、capture data、動作する exploit を public issue へ投稿しないでください。

Security workflow は tracked file の一般的な committed secret marker を scan し、Rust の CodeQL と `cargo audit` を実行します。これらの検査は protocol change と deployment control の review を補完するものであり、置き換えるものではありません。

## Remote control と password

`--allow-input` は接続済み client に local keyboard / pointer の操作を許可します。既定では無効です。loopback listener または別の access control を備えた trusted private network でだけ有効化してください。

`[auth]` がない場合、kVNC は意図的に password 無しです。設定した場合は RA2ne（SecurityType 6）を使用します。ephemeral RSA/AES handshake で username/password exchange を暗号化し、保存するのは Argon2id PHC password verifier のみです。`bin/kvnc --generate-password-hash` で生成し、plaintext password を user configuration file に書かないでください。

classic VNC password authentication（SecurityType 2）は reversible な DES 由来 key を使い password を8 byteに切り詰めるため、引き続き未対応です。RA2ne は credential を保護しますが、login 後の RFB session は暗号化しません。trusted encrypted overlay network または SSH tunnel を使用し、input を有効化した listener を untrusted network へ公開しないでください。

RA2ne RSA key pair は kVNC process の起動時に一度だけ生成され、その process 内のすべての connection で共有されます。debug log は生成時と connection phase に公開 wire value の SHA-256 fingerprint だけを記録します。fingerprint が変わるのは connection ごとの rekey ではなく、新しい kVNC process が起動したことを示します。
