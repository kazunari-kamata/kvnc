# RA2ne / noVNC 相互運用調査

この文書は、kVNC と noVNC の RA2ne（RFB SecurityType `6`）接続が、credential 送信前に `RA2: corrupted server encrypted random` で失敗した事象の引継ぎ用です。秘密鍵、random、password、clipboard 本文は記録していません。

## 結論（現在）

kVNC は noVNC が要求する RA2ne wire format で server random を送信していることを確認済みです。更新済み kVNC の live listener に対し、WebCrypto で生成した client RSA key を使い、noVNC legacy RSA と同じ BigInt modular private-key operation と PKCS#1 v1.5 unpadding で response を検証しました。結果は `00 02` padding と16-byte plaintext でした。

最新の失敗時、kVNC は client public key と client random を正常に処理した後に 256-byte server-random ciphertext を送信し、約76 ms後に client 側から EOF を受けました。したがって、調査対象は bridge/noVNC runtime が送信した public key と保持する private key の対応、または local TCP bridge と WebSocket 間の binary payload integrity です。

この結論は bridge/noVNC の単独不具合を断定するものではありません。両実装の接続ごとの状態と wire payload を安全に照合する必要があります。

## kVNC の実装と確認済み事項

| 項目 | 確認結果 |
| --- | --- |
| RFB version | noVNC は `003.008` を提示 |
| Security type | `6`（RA2ne）を選択 |
| Server public key | U32 `2048`、modulus `n` 256 bytes、exponent `e` 256 bytes、big-endian |
| Client public key | U32 `2048`、`n` / `e` 各256 bytesで受信 |
| Client random | 256-byte RSAES-PKCS1-v1_5 ciphertext を kVNC private key で復号成功 |
| Server random | 16 random bytesを client public keyで RSAES-PKCS1-v1_5 encryption。U16 `256` と256-byte ciphertext を送信 |
| Local verification | noVNC legacy RSA 相当の JWK `d` / BigInt / PKCS#1 v1.5 decrypt で16 bytesへ復号成功 |
| Password | server random の復号失敗時点では credential packet 未到達。原因ではない |

RA2ne の credential exchange の仕様は [RA2ne Security Type Specification](https://gist.github.com/pdlan/81245e3d83d7fa586b8dcbb4e2d507cc)、noVNC の実装順序は [noVNC `core/ra2.js`](https://github.com/novnc/noVNC/blob/master/core/ra2.js) を参照してください。

## kVNC の connection-scoped diagnostics

更新済み kVNC は `--debug` と `log_file` 有効時、公開情報とサイズだけを記録します。

```text
RA2ne server key generated: fingerprint_sha256=<fingerprint>
RA2ne: phase=server-public-key-send fingerprint_sha256=<fingerprint>
RA2ne: received client public key bits=2048 modulus_bytes=256 exponent_bytes=256
RA2ne: peer=<peer> direction=client-to-server packet=client-public-key bytes=516 sha256=<hex>
RA2ne: received encrypted client random bytes=256
RA2ne: phase=client-random-decrypted fingerprint_sha256=<fingerprint>
RA2ne: peer=<peer> direction=server-to-client packet=server-encrypted-random bytes=256 sha256=<hex>
RA2ne: phase=authentication-complete fingerprint_sha256=<fingerprint>
```

RSA server key pair は kVNC process 起動時に一度だけ生成され、同一 process 内の全 connection が共有します。上記 fingerprint は process 内で一定です。fingerprint が変わるのは kVNC 再起動後です。

`client-public-key` と `server-encrypted-random` の SHA-256 は opaque binary payload の fingerprint です。public key wire value と RSA ciphertext の raw value、private key、random、password を出さずに bridge/noVNC の同一 connection の log と照合するために使用します。

## Live capture の照合結果（2026-11-04）

同じ kVNC process-scoped server key fingerprint `6b:85:04:81:…:4c:9f` の connection で、noVNC が記録した ClientPublicKey hash と kVNC が受信した hash は一致しました。

```text
ClientPublicKey SHA-256
noVNC: e92e7dd2df18bb49bdb7323c0408b1678aa00bd6852c799ae974ccdb5c70ed33
kVNC:  e92e7dd2df18bb49bdb7323c0408b1678aa00bd6852c799ae974ccdb5c70ed33
```

一方、同じ connection の ServerRandom ciphertext hash は一致しませんでした。

```text
ServerRandom ciphertext SHA-256
noVNC: 76b3c94be54f1136d988e2a93730833078364a2deda4a22b2a92456376193f46
kVNC:  1a7bceca36fe9d6cf4ad9c491580e36f5162a28979ff2349be1cca15bffb6035
```

RSAES-PKCS1-v1_5 ciphertext は毎回新しい random padding を含むため、kVNC の hash が接続ごとに変化するのは期待どおりです。ClientPublicKey の一致は connection correlation と kVNC 側の受信 bytes を確認しますが、ServerRandom hash の不一致は kVNC が TCP socket に書いた256 bytesと noVNC が receive queue から読んだ256 bytesが同一でないことを示します。次の調査対象は bridge の server→client binary relay、WebSocket frame ordering、または noVNC receive queue の stale frame です。

成功時は `phase=authentication-complete` と `client connected` が続きます。今回の failure は `sent encrypted server random bytes=256` の直後に client EOF となり、authentication-complete には到達しませんでした。

## noVNC / bridge 側で確認してほしいこと

1. `serververification` を user が承認した connection と、RA2ne state object が同一であること。
2. client RSA key pair を一度だけ生成し、wire に送った `n` / `e` と server-random decrypt に使う private key が同一 pair であること。
3. local TCP bridge が server→client の `U16 256 + 256-byte ciphertext` を WebSocket binary frame として byte-for-byte 転送すること。文字列化、UTF-8 conversion、base64 conversion、chunk boundary の誤処理を行わないこと。
4. `serverEncryptedRandom` を受信する直前の read queue に、前段の client-random write や別 connection の bytes が混入していないこと。
5. noVNC の復号失敗時、秘密値を出さずに次だけを log すること: connection identifier、server-random ciphertext length、decrypt result が `null` か plaintext length 不一致か、RA2ne state identity。

## 再現手順

1. kVNC を password-only auth と debug log 有効で起動する。

   ```sh
   bin/kvnc
   tail -f ~/.config/kvnc/monitor-0.log
   ```

2. noVNC bridge から RFB 3.8 / SecurityType 6 を接続し、表示された server-key fingerprint を user が承認する。
3. kVNC log の `client-public-key` / `server-encrypted-random` の bytes と SHA-256、server key fingerprint、phase を保存する。
4. noVNC 側の同一 connection identifier と時刻の SHA-256 を対応付ける。client public key hash が一致しなければ client/bridge の framing または state を、server-random ciphertext hash が一致しなければ TCP/WebSocket bridge の binary relay を調査する。両方一致して decrypt result が `null` の場合は、実際に受信した client public key を用いる RSA encryption と noVNC legacy RSA decrypt の組を重点調査する。

## Security requirements

- RA2ne、server-key verification、RSA encryption を無効化して回避しない。
- password、private key、random、clipboard 本文を log や issue に出さない。
- binary payload を比較する必要がある場合も、まず size / state identity / stage を確認し、必要な場合だけ別途安全な手順を定義する。
