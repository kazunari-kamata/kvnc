# 検証ガイド

English: [verification.en.md](verification.en.md)

protocol、listener、clipboard、desktop capture、background operation の変更には、この手順を使用します。network / RFB の失敗と OS の capture permission を混同しないよう、必ず `--source demo` から開始します。

## 1. local protocol の確認

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
mkdir -p var
bin/kvnc --source demo --listen 127.0.0.1:5900 \
  --debug --log-file "$PWD/var/kvnc.log"
```

同一 machine 上の互換 RFB client を接続します。log には `client accepted`、`client connected`、`FramebufferUpdateRequest` があり、`ERROR` がないことを確認します。

既定 server は SecurityType None を使用します。汎用 client UI が username / password field を表示する場合は、両方を空欄にします。password login を検証する場合は `bin/kvnc --generate-password-hash` で使い捨て verifier を生成し、test username とともに `[auth]` へ設定して再起動します。client が RA2ne を選び、設定した credential だけを受け入れることを確認します。password 無しの運用に戻す場合は test の `[auth]` section を削除します。

handshake を変更した場合は、RFB `003.008` と `003.003` の両方を検証します。3.3 client では `handshake: RFB 3.3 security_type=None`、modern client では `handshake: RFB 3.7+ security_type=None` が記録される必要があります。

## 2. trusted remote listener の確認

別 machine の client から接続する場合は、`0.0.0.0` ではなく kVNC host に割り当てられた特定 private-network address を使用します。

```sh
bin/kvnc --source demo --listen SERVER_PRIVATE_IP:5900 \
  --debug --log-file "$PWD/var/kvnc.log"
lsof -nP -iTCP:5900 -sTCP:LISTEN
```

RFB client を開く前に、client host から TCP 到達性を確認します。

```sh
nc -vz -w 3 SERVER_PRIVATE_IP 5900
```

次に、exact target が `tcp://SERVER_PRIVATE_IP:5900` の RFB client を接続します。接続中は server で `tail -f var/kvnc.log` を実行します。

| Server log の結果 | 意味と次の操作 |
| --- | --- |
| `client accepted` がない | client target、exact bind address、private-network ACL、host firewall を確認します。 |
| `client accepted` の後に `client connected` | TCP と RFB negotiation は成功です。次に framebuffer request を確認します。 |
| `client disconnected` | interoperability report 用に message type と reason を保存します。 |
| `FramebufferUpdateRequest` と `framebuffer update sent` | client は RFB framebuffer response を受信しています。 |

password 無しの RFB は encryption / authentication を提供しません。RA2ne は設定した credential exchange だけを保護し、以後の session は保護しません。この手順を信頼できない network で使用してはいけません。

## 3. clipboard の確認

demo source への接続中に、破棄可能な plain text で確認します。

1. client で text を copy し、kVNC host の local editor に paste します。
2. kVNC host の local editor で別の text を copy し、client に paste します。
3. Extended Clipboard 対応 client では `extended clipboard capabilities received`、`extended clipboard received` が記録されることを確認します。非対応 client では legacy clipboard の byte count が記録されます。
4. `var/kvnc.log` に clipboard 本文が記録されないことを確認します。

## 4. desktop capture の確認

demo test が成功してから、interactive desktop login で `--source desktop` を実行します。macOS では **System Settings → Privacy & Security → Screen Recording** で launcher を許可して再起動し、`sudo` は使用しません。server が `no monitor is available` と報告したら protocol の調査を中止してください。host に capture 可能な desktop context、display、または permission がありません。

画面が一様な黒になる場合は、まず Mac を unlock して display を wake します。`system_profiler SPDisplaysDataType` の各 display に `Display Asleep: Yes` が表示される間は、macOS が黒い capture image を返すことがあります。連続運用中に sleep を抑止する必要がある場合は、trusted local session で `caffeinate -dimsu` を実行し、終了時に `Ctrl-C` で停止します。

## 5. background operation の確認

macOS / Linux では、`--daemon` が listener を残し、指定した PID と log を書き込むことを確認します。macOS desktop capture は、文書化された LaunchAgent を使用してください。LaunchAgent の arguments に `--daemon` は追加しません。[バックグラウンド運用](daemon.ja.md) を参照してください。

## 6. remote input と複数 monitor の確認

`--list-monitors` で stable な zero-based index を確認します。monitor ごとに別の kVNC process と listener port を起動します。これにより display ごとに独立した RFB framebuffer を提供します。

```sh
bin/kvnc --list-monitors
bin/kvnc --source desktop --monitor 0 --allow-input --listen 127.0.0.1:5900
bin/kvnc --source desktop --monitor 1 --allow-input --listen 127.0.0.1:5901
```

pointer movement、left / middle / right button、wheel scroll、text key、modifier key、navigation key、F1–F12 を確認します。macOS ではこの test の前に起動元へ Accessibility permission を許可してください。untrusted network では input を有効にしないでください。

Retina の primary display では、kVNC は selected monitor の scale factor を使用して captured framebuffer pixel を macOS の論理 input point へ変換します。各 corner 付近を click して、対応する local screen の位置へ操作されることを確認してください。

### drag selection の切り分け

新しい debug log から開始し、破棄可能な text 上で left button の slow drag を1回だけ行います。kVNC は millisecond 精度で記録します。正常な drag は、最初の `mask=0x01`、座標が変化する1件以上の後続 `mask=0x01`、最後の `mask=0x00` で構成されます。各 `client message: PointerEvent` の直後には、同じ座標を持つ `pointer event injected` が記録される必要があります。button を保持した event がなければ client または bridge が drag を送っていません。保持 event と即時の injection があるにもかかわらず macOS の選択位置だけが違う場合は、その該当 log を kVNC input 調査に使用します。

```sh
rg -n 'PointerEvent|pointer event injected' var/kvnc.log | tail -n 80
```
