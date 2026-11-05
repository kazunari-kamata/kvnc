# バックグラウンド運用

English: [daemon.en.md](daemon.en.md)

## Terminal からの foreground 起動

`--source desktop` の初回起動と permission 許可は、Mac 本体の sign-in 済み GUI session で開いた Terminal.app / iTerm2 から行います。SSH shell や remote client host の Terminal ではありません。`~/.config/kvnc/config.toml` を設定済みなら、working directory に関係なく引数は不要です。

```sh
/Users/YOUR_USER/path/to/kvnc/bin/kvnc
```

foreground process は `Control-C` で停止し、同じ command で起動し直します。macOS の Screen Recording / Accessibility permission はこの local GUI launch context に許可してください。`sudo` は使用しません。

## macOS / Linux の config daemon

`~/.config/kvnc/config.toml` の `daemon = true` は kVNC を現在の terminal から切り離して background で実行します。`log_file` が必須です。`pid_file` は停止対象を明確にします。

```toml
# ~/.config/kvnc/config.toml
daemon = true
log_file = "/Users/YOUR_USER/path/to/kvnc/var/kvnc.log"
log_rotation = "daily"
log_max_size_bytes = 104857600
log_retention = 14
pid_file = "/Users/YOUR_USER/path/to/kvnc/var/kvnc.pid"
```

```sh
mkdir -p var
/Users/YOUR_USER/path/to/kvnc/bin/kvnc
```

動作確認と停止は次のとおりです。

```sh
tail -f var/kvnc.log
kill "$(cat var/kvnc.pid)"
```

automation では absolute path の log-file と pid-file を使用してください。config daemon は macOS と Linux で利用できます。Windows では foreground の `kvnc` process を管理する service manager を使用してください。

kVNC 自身が `log_file` の rotation を行います。以前の file は `kvnc.log.<Unix-timestamp>` として archive し、元の path に新しい active file を作成します。`log_rotation` は `none`、`hourly`、`daily`、`weekly`、`monthly` を指定できます。`monthly` は calendar month 境界ではなく30日周期です。`log_max_size_bytes` が0以外なら size 到達時にも早期 rotation します。`log_retention` は残す archive 数です。foreground、daemon、LaunchAgent のいずれでも process の reopen / restart は不要です。

## macOS の desktop capture: LaunchAgent を使う

`--source desktop` は root LaunchDaemon や `sudo` ではなく、sign-in 中 user の **LaunchAgent** として起動してください。Screen Recording permission は sign-in 中 desktop user の process identity に付与されるため、system daemon は利用できません。

`~/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist` を作成します。以下の path と trusted listener address は環境に合わせて置換してください。

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.kazunari-kamata.kvnc</string>
  <key>ProgramArguments</key><array>
    <string>/Users/YOUR_USER/path/to/kvnc/bin/kvnc</string>
  </array>
  <key>WorkingDirectory</key><string>/Users/YOUR_USER/path/to/kvnc</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/Users/YOUR_USER/path/to/kvnc/var/launchd.out.log</string>
  <key>StandardErrorPath</key><string>/Users/YOUR_USER/path/to/kvnc/var/launchd.err.log</string>
</dict></plist>
```

log directory を作成し、sign-in 中 user の GUI session で load します。

```sh
mkdir -p /Users/YOUR_USER/path/to/kvnc/var
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist"
launchctl kickstart -k "gui/$(id -u)/com.kazunari-kamata.kvnc"
```

user configuration の `~/.config/kvnc/config.toml` が引数なしで読まれます。`WorkingDirectory` は configuration を選びません。LaunchAgent を使う場合は config の `daemon` を有効にしないでください。launchd が restart と process lifecycle を管理します。**System Settings → Privacy & Security → Screen Recording** で起動元を許可し、permission を変更した場合は agent を再起動してください。削除する場合は `launchctl bootout "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist"` を実行します。

## SSH 経由での起動

SSH からも `--source demo` は直接起動できます。`sudo` は使用しません。次の command は config に `source = "demo"` を設定した場合のものです。config daemon を使う場合は SSH を閉じても process が残ります。

```sh
ssh YOUR_USER@MAC_HOST '/Users/YOUR_USER/path/to/kvnc/bin/kvnc'
```

`--source desktop` は、SSH shell に利用可能な desktop capture context がなく、macOS の Screen Recording consent prompt に応答できない場合が多いため、SSH からの直接起動には向きません。login 中 Mac の desktop で LaunchAgent を一度作成・許可してから、SSH ではその user agent の制御だけを行ってください。

```sh
ssh YOUR_USER@MAC_HOST 'launchctl kickstart -k "gui/$(id -u)/com.kazunari-kamata.kvnc"'
ssh YOUR_USER@MAC_HOST 'launchctl print "gui/$(id -u)/com.kazunari-kamata.kvnc"'
ssh YOUR_USER@MAC_HOST 'tail -f /Users/YOUR_USER/path/to/kvnc/var/kvnc.log'
```

同じ user の GUI login session が有効であることが必要です。desktop session または monitor がない場合、`--source desktop` は framebuffer を取得できません。transport の切り分けには `--source demo` を使用してください。
