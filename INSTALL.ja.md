# 導入

English: [INSTALL.md](INSTALL.md)

## 必要な環境

- Rust stable toolchain
- `--source desktop` を使う場合は desktop session
- OS が要求する desktop capture 権限

Linux では desktop capture support の compile に必要な development package を導入します。

```sh
sudo apt-get install libdbus-1-dev libpipewire-0.3-dev libwayland-dev libx11-dev libxrandr-dev
```

## Standalone executable の作成

```sh
cargo install --path . --root . --force --locked
bin/kvnc --version
```

`bin/kvnc` は現在の platform 用 native executable です。実行時に Cargo は不要です。

Windows では PowerShell から `.\bin\kvnc.exe` を実行します。clone、設定、update の全手順は [Windows ガイド](docs/windows.ja.md) を参照してください。

## 起動

```sh
bin/kvnc --source desktop --listen 127.0.0.1:5900 --debug --log-file kvnc.log
```

desktop capture を使わず RFB / clipboard の処理を確認する場合は `--source demo` を指定します。localhost 外へ公開する前に [対応環境](docs/compatibility.ja.md) を確認してください。

### macOS の権限と `sudo`

kVNC を `sudo` で実行する必要はなく、実行しないでください。desktop を capture する process は、sign-in している desktop user である必要があります。`sudo` を使うと root process は別の privacy permission identity になり、通常の user に許可した権限を利用できません。**System Settings → Privacy & Security → Screen Recording** で kVNC を起動する application（Terminal や IDE など）を許可し、その application を終了してから開き直してください。standalone binary を直接起動した場合は、一覧に `kvnc` と表示されることもあります。

keyboard / pointer の操作を許可するには `--allow-input` を追加します。macOS ではさらに **System Settings → Privacy & Security → Accessibility** で起動元 application を許可し、起動元を再起動してください。`--debug --log-file kvnc.log` を指定すると、clipboard 本文を保存せずに permission と RFB 接続の失敗を記録できます。

### 別の trusted host から接続する場合

既定の `127.0.0.1:5900` は同一 machine 上の client だけを受け付けます。client が別 host にある場合は、全 interface ではなく trusted private network interface の特定 address に kVNC を bind してください。`SERVER_PRIVATE_IP` を server の trusted overlay address に置き換えて起動します。

```sh
bin/kvnc --source demo --listen SERVER_PRIVATE_IP:5900 --debug --log-file kvnc.log
```

まず demo source で接続を確認してから、`demo` を `desktop` に置き換えます。v0.1.0 の RFB は encryption / authentication を提供しないため、この listener を信頼できない network に公開しないでください。

`--daemon` と macOS launchd user agent による運用は [バックグラウンド運用](docs/daemon.ja.md) を参照してください。
