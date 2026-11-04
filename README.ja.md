# kVNC

kVNC は、macOS、Windows、Linux 向けの Rust 製 CLI VNC サーバーです。RFB 3.8 の Raw / Hextile encoding、legacy clipboard (`ClientCutText` / `ServerCutText`)、Extended Clipboard の text format を実装し、プレーンテキストをシステム clipboard と双方向同期します。

English: [README.md](README.md)

Repository: [kazunari-kamata/kvnc](https://github.com/kazunari-kamata/kvnc)  
GitHub: [kazunari-kamata](https://github.com/kazunari-kamata) (Kazunari Kamata)

## 目的と範囲

kVNC は native OS API で選択した desktop を取得し、設定した RFB endpoint で公開します。local GUI を持たない CLI server です。keyboard / pointer の注入は既定で無効です。trusted client に限り、OS の必要な permission を許可したうえで `--allow-input` により明示的に有効化します。

`[auth]` 有効時の RA2ne credential exchange を除き、RFB traffic は暗号化されません。既定では `127.0.0.1:5900` に bind します。transport security と network access control を設計するまでは、この loopback-only 設定を維持してください。

## 導入と起動

Windows の Git clone、PowerShell command、desktop session の注意点、保守手順は [Windows ガイド](docs/windows.ja.md) を参照してください。Windows の native binary は `bin\kvnc.exe` であり、`.\bin\kvnc.exe` として実行します。

現在の platform 用 native standalone binary を作成します。

```sh
cargo install --path . --root . --force --locked
bin/kvnc --version
bin/kvnc --source desktop --listen 127.0.0.1:5900 --debug --log-file kvnc.log
```

作成された `bin/kvnc` は Cargo を介さず直接実行できます。OS から求められた場合は desktop capture 権限を許可してください。

## 設定ファイルによる起動

引数なし起動時は repository 内ではなく user ごとの configuration file を読みます。macOS / Linux では `${XDG_CONFIG_HOME:-~/.config}/kvnc/config.toml`（通常は `~/.config/kvnc/config.toml`）、Windows では `%APPDATA%\kvnc\config.toml` です。`kvnc --init` で comment 付きの安全な既定 file を作成してから、特に `listen` と `allow_input` を見直してください。これにより listener address、log path、password verifier が Git の対象になりません。

```sh
bin/kvnc
```

user configuration の自動読込は、意図的に引数なし起動だけに限定しています。`bin/kvnc --source desktop ...` のように option を渡す command は CLI default を使い、`[auth]` を読みません。設定済み server は `bin/kvnc` で起動するか、`--config /path/to/kvnc.toml` を明示してください。

project-local file を含む別の file を使う場合は明示します。

```sh
bin/kvnc --config /path/to/kvnc.toml
```

設定 file を使わず引数なしで実行した場合は、従来どおり `127.0.0.1:5900`、desktop source、input 無効で起動します。

### 設定項目と CLI option

| TOML key | 対応する CLI | 値・既定値 |
| --- | --- | --- |
| `listen` | `--listen` | socket address。既定 `127.0.0.1:5900` |
| `source` | `--source` | `desktop` または `demo`。既定 `desktop` |
| `monitor` | `--monitor` | `--list-monitors` の zero-based index。省略時 primary display |
| `allow_input` | `--allow-input` | boolean。既定 `false` |
| `keyboard_layout` | `--keyboard-layout` | macOS input 用の `auto`、`us`、`jis`。既定 `auto` |
| `scale` | `--scale` | `0.1` から `1.0`。既定 `1.0` |
| `fps` | `--fps` | `1` から `10`。既定 `2` |
| `debug` | `--debug` | boolean。既定 `false` |
| `log_file` | `--log-file` | relative / absolute log path |
| `log_rotation` | `--log-rotation` | `none`（既定）、`hourly`、`daily`、`weekly`、`monthly` |
| `log_max_size_bytes` | `--log-max-size-bytes` | この size で rotate。`0`（既定）は size 制限なし |
| `log_retention` | `--log-retention` | 保持する timestamp 付き archive file 数。既定 `14`、`0` は保持なし |
| `daemon` | `--daemon` | boolean。macOS/Linux のみ。`log_file` が必須 |
| `pid_file` | `--pid-file` | pid path。`daemon = true` が必須 |
| `[auth].username` | — | optional login name。省略時は password-only login |
| `[auth].password_hash` | `--generate-password-hash`（生成のみ） | plaintext password ではない Argon2id PHC verifier |

`--help`、`--version`、`--list-monitors`、`--print-config`、`--config <PATH>`、`--init`、`--update-config`、`--update` / `--upgrade` は実行時専用のため TOML key はありません。`--print-config` は有効な設定を表示し、password verifier は伏せ字にします。`--update-config` は timestamp 付き backup を作成し、既存 value を置き換えずに未設定の安全な default を追加します。configuration file の unknown key は error になります。

daemon を使うときは `daemon = true` に変更します。`log_file` と `pid_file` を設定したうえで、同じく引数なしで起動できます。

```toml
daemon = true
pid_file = "kvnc.pid"
```

kVNC は external log manager が開いた file を移動しなくてもよいよう、log を自身で rotation できます。周期または size 上限のうち先に到達した条件で rotate し、active file の path は `kvnc.log` のままです。archive は `kvnc.log.<Unix-timestamp>` という名前になります。`monthly` は calendar month の境界ではなく、前回 rotation から30日後です。例:

```toml
log_file = "/absolute/path/kvnc.log"
log_rotation = "daily"
log_max_size_bytes = 104857600 # 100 MiB。size rotation を無効にする場合は 0
log_retention = 14
```

desktop capture 権限なしで RFB と clipboard だけを確認する場合は、demo source を使います。

```sh
bin/kvnc --source demo --listen 127.0.0.1:5900
```

RFB 3.8 対応 VNC client から `127.0.0.1:5900` に接続します。

`--debug` は RFB handshake と client message を記録します。clipboard の内容自体は記録しません。`--log-file` を指定すると同じ出力を file に追記するため、client の切断調査に利用できます。Extended Clipboard 対応 client とは capability negotiation 後に Notify / Request / Provide を使い、UTF-8 text を zlib 圧縮して双方向同期します。非対応 client には従来どおり `ClientCutText` / `ServerCutText` を使います。

v0.1.0 は client が Hextile を提示した場合に Hextile を選択し、それ以外は Raw を使用します。Hextile は source pixel を落とさない lossless encoding なので、`scale` を下げずに desktop の平坦な部分の転送量を削減できます。初回は全画面 frame を送り、その後の incremental request には変更された最小 rectangle だけを送ります。framebuffer update は最大 1 件まで queue し、`--fps` を上限として capture します。keyboard / pointer event は別処理のため、画面転送が重なっても input event は待たせません。通常は `--fps 5`、帯域を優先する場合は `--fps 1` を使います。

Hextile を利用しても画面反映が遅い場合にだけ `--scale 0.5` を指定します。Retina の 2880x1800 capture は 1440x900 になり、最悪時の転送量はおよそ4分の1です。`scale = 1.0` は native capture resolution を維持して文字の鮮明さを優先します。input coordinate は縮小後の framebuffer に合わせて自動変換されます。

```sh
bin/kvnc --source desktop --monitor 0 --allow-input --scale 0.5 \
  --listen 127.0.0.1:5900
```

既定の kVNC は RFB SecurityType None を使用するため、username / password は不要です。client 側が credential field を表示しても、両方を空欄にしてください。access は loopback または trusted encrypted private network に制限します。

username / password login を有効化する場合は、interactive に Argon2id verifier を生成します。password は echo も保存もされません。出力された値を user configuration file の `[auth]` に設定してください。

```sh
bin/kvnc --generate-password-hash
```

```toml
[auth]
username = "operator"
password_hash = "$argon2id$..."
```

password のみで login する場合は `username` を省略します。kVNC は RA2ne の password-only credential subtype を要求します。

`[auth]` を設定すると kVNC は RFB RA2ne authentication を提示します。ephemeral RSA/AES handshake で username/password exchange を暗号化し、保存済み Argon2id verifier と照合します。RA2ne は以後の framebuffer、input、clipboard traffic を暗号化しません。trusted encrypted private network または SSH tunnel は継続して使用してください。server key は起動時生成のため、client が server key の承認を求める場合があります。

monitor を一覧・選択できます。画面ごとに別 VNC view が必要な場合は、monitor ごとに kVNC process を起動します。

```sh
bin/kvnc --list-monitors
bin/kvnc --source desktop --monitor 1 --allow-input --listen 127.0.0.1:5901
```

macOS の `--allow-input` には、起動元 application の Accessibility permission も必要です。

macOS では RFB の printable keysym を physical key code として注入します。これは UTF-8 の変換ではなく JIS keyboard と IME の key binding を保持するためです。`keyboard_layout = "jis"`（または `--keyboard-layout jis`）は JIS の `@`（濁点）、`[`（半濁点）、`^`（へ）、`¥`（長音）、`ろ`、`け`、`む`の keysym をそれぞれ macOS JIS key code へ変換し、「日本語 - かな入力」での直接かな入力に使用します。通常の配列では既定の `auto` を使います。debug log には keysym 値だけを残し、入力文字列は記録しません。

## 文書

- [導入](INSTALL.ja.md)
- [Windows ガイド](docs/windows.ja.md)
- [プロトコルと security model](docs/protocol.ja.md)
- [対応環境](docs/compatibility.ja.md)
- [バックグラウンド運用](docs/daemon.ja.md)
- [検証ガイド](docs/verification.ja.md)
- [Security policy](docs/security.ja.md)
- [RA2ne / noVNC 相互運用調査](docs/ra2ne-novnc-investigation.ja.md)
- [貢献方法](CONTRIBUTING.ja.md)
- [変更履歴](CHANGELOG.md)

## License

[MIT](LICENSE)
