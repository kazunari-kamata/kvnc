# 起動・停止・再起動と開発時の運用

English: [control.en.md](control.en.md)

この文書では、`caffeinate` の3種類の用途を区別し、1つの kVNC instance を
安全に start / stop / restart / status / log 確認する方法を示します。すべて
sign-in 中の desktop user で実行し、`sudo` は使いません。

## `caffeinate` は何を停止するか

| 用途 | 継続時間 | 正しい停止方法 |
| --- | --- | --- |
| LaunchAgent wrapper: `scripts/kvnc-launch-macos.sh`（一度だけの `caffeinate -u -t 5`、続いて `caffeinate -i`） | LaunchAgent job が load されている間。display assertion は起動時の5秒間だけ | `caffeinate` を直接 kill せず LaunchAgent を unload する |
| Terminal から手動実行した `caffeinate -dimsu` | Terminal process が終了するまで | その Terminal で `Control-C`、または起動した PID を確認して signal を送る |
| 接続時 wake: `caffeinate -u -t 5` の完了後、client 単位の `caffeinate -d` | 認証済み VNC 接続からその client の切断まで | VNC client を切断する。kVNC が assertion を解除する |

LaunchAgent の例では `KeepAlive = true` を使います。そのため子 process の
`caffeinate` を kill しても、`launchd` が再起動します。job 自体を停止すると
wrapper と kVNC を同時に止め、再起動もしません。

```sh
launchctl bootout "gui/$(id -u)/com.kazunari-kamata.kvnc"
```

plist は `~/Library/LaunchAgents/` に残り、この command で削除されません。
他 application の power assertion まで止める `pkill caffeinate` / `killall
caffeinate` は使わないでください。

## `kvncctl` による簡易操作

repository には次の helper を追加します。

- macOS/Linux: `scripts/kvncctl`
- Windows Command Prompt: `scripts\kvncctl.cmd`（`scripts\kvncctl.ps1` を呼ぶ薄い launcher）
- Windows PowerShell: PowerShell から直接使う `scripts\kvncctl.ps1`

helper は安全に特定できる instance だけを制御します。

- macOS の LaunchAgent mode は
  `gui/<uid>/com.kazunari-kamata.kvnc` だけを制御します。
- macOS/Linux の daemon mode は指定 PID file 内の PID だけを対象にし、その
  PID が期待する `bin/kvnc` path の process であることを確認します。
- Windows PowerShell helper は自身が作った PID file だけを使い、
  `Win32_Process` で executable path を確認します。

関連のない foreground kVNC、未知の PID、他 application の `caffeinate` を
kill することはありません。`status` は port 5900（または `--port` で指定した
port）の listener も表示するため、既存 server の確認に使えます。

### macOS LaunchAgent

[バックグラウンド運用](daemon.ja.md) の標準 plist を作成した後は、次を使います。

```sh
scripts/kvncctl status
scripts/kvncctl stop
scripts/kvncctl start
scripts/kvncctl restart
```

mode を指定しない macOS では、
`~/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist` があれば LaunchAgent
mode を選びます。`stop` は `launchctl bootout`、`start` は必要に応じて plist の
bootstrap、`restart` は binary / config / permission 変更後の安全な再起動です。
active kVNC log が既定と異なる場合は、次のように log を追跡できます。

```sh
KVNC_LOG_FILE=/Users/YOUR_USER/path/to/kvnc/var/kvnc.log scripts/kvncctl logs
```

helper を使わず agent を確認する場合は次の command です。

```sh
launchctl print "gui/$(id -u)/com.kazunari-kamata.kvnc"
```

### macOS/Linux の daemon

config file で daemon を運用する場合、`log_file` は必須です。helper は
`--daemon` と `--pid-file` を追加するため、file に `daemon = true` を保存する
必要はありません。運用 instance には absolute log / PID path を設定します。

```toml
log_file = "/Users/YOUR_USER/path/to/kvnc/var/kvnc.log"
# scripts/kvncctl から起動する場合、`daemon = true` は省略可能。
```

```sh
scripts/kvncctl --daemon \
  --config "$HOME/.config/kvnc/config.toml" \
  --pid-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.pid \
  --log-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.log \
  status
scripts/kvncctl --daemon \
  --config "$HOME/.config/kvnc/config.toml" \
  --pid-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.pid \
  --log-file /Users/YOUR_USER/path/to/kvnc/var/kvnc.log \
  restart
```

Linux で長時間 desktop capture を使う場合は、文書化済みの graphical-session
`systemd --user` unit を優先します。lifecycle は systemd が所有するため、この
unit には `kvncctl` ではなく `systemctl --user restart kvnc.service` を使います。

### Windows Command Prompt と PowerShell

sign-in 済み interactive desktop user session の Command Prompt から wrapper を実行します。

```cmd
scripts\kvncctl.cmd status
scripts\kvncctl.cmd stop
scripts\kvncctl.cmd start
scripts\kvncctl.cmd restart
scripts\kvncctl.cmd logs
```

`.cmd` wrapper は `-NoProfile` で PowerShell を起動し、確認済みの `.ps1` 実装へ全引数を渡します。`-ExecutionPolicy Bypass` は repository script の実行 policy だけを当該起動に限って回避するもので、kVNC authentication を bypass したり administrator 権限を与えたりするものではありません。PowerShell parameter や対話的な切り分けを使う場合は `.ps1` を直接実行できます。helper は user config を指定して `bin\kvnc.exe` を直接起動し、PID を `var\` に
記録します。これは開発時の簡易操作用であり、文書化済みの Task Scheduler に
よる logon task の代替ではありません。同一 listener に両方を起動しないで
ください。PowerShell から直接実行する場合は、内容を確認してから次のように
実行できます。

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\kvncctl.ps1 status
```

## 既存 server と開発作業

開発 server を起動する前に、listener を所有している server を確認します。

```sh
scripts/kvncctl status
```

標準 LaunchAgent なら `scripts/kvncctl stop` で停止してから build / update を
行い、`scripts/kvncctl restart` または `start` を実行します。helper で起動した
daemon は、同じ `--pid-file` を指定して `--daemon stop` を実行します。`status`
が listener を表示する一方で unmanaged と表示した場合は、表示された PID /
command を確認してから対応してください。代表的な安全な停止方法は次のとおりです。

- Terminal で foreground 起動した server: その Terminal へ戻って `Control-C`
- 既知の LaunchAgent: 前述の `launchctl bootout`
- 別管理の `systemd --user` service: `systemctl --user stop kvnc.service`

同じ address / port に2つ目の server を起動してはいけません。production server
を動かしたまま development build を試す場合は、別 listener、PID file、log file
を使う private な `config.dev.toml` を作ります。`config.*.toml` は Git の対象外です。

```toml
# config.dev.toml
listen = "127.0.0.1:5901"
source = "demo"
allow_input = false
debug = true
log_file = "var/kvnc-dev.log"
```

foreground での開発時は次のように起動します。

```sh
bin/kvnc --config config.dev.toml
```

別 daemon として管理する場合は、明示的に別の path / port を指定します。

```sh
scripts/kvncctl --daemon --config config.dev.toml \
  --pid-file var/kvnc-dev.pid --log-file var/kvnc-dev.log --port 5901 start
```

まず `source = "demo"` で確認してください。desktop capture を並行して試す場合も、
意図した試験でなく、同じ desktop を他の remote operator が操作している間は remote
input を無効のままにします。
