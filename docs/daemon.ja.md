# バックグラウンド運用

English: [daemon.en.md](daemon.en.md)

## Terminal からの foreground 起動

`--source desktop` の初回起動と permission 許可は、Mac 本体の sign-in 済み GUI session で開いた Terminal.app / iTerm2 から行います。SSH shell や remote client host の Terminal ではありません。`~/.config/kvnc/config.toml` を設定済みなら、working directory に関係なく引数は不要です。

```sh
/Users/YOUR_USER/path/to/kvnc/bin/kvnc
```

foreground process は `Control-C` で停止し、同じ command で起動し直します。macOS の Screen Recording / Accessibility permission はこの local GUI launch context に許可してください。`sudo` は使用しません。

既存 LaunchAgent / daemon の安全な操作と、別 listener による development instance は、[起動・停止・再起動と開発時の運用](control.ja.md)を参照してください。

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

automation では absolute path の log-file と pid-file を使用してください。config daemon は macOS と Linux で利用できます。Windows では desktop capture 用の Windows service ではなく、sign-in 済み interactive user session でのみ動作する logon task を使用してください。

kVNC 自身が `log_file` の rotation を行います。以前の file は `kvnc.log.<Unix-timestamp>` として archive し、元の path に新しい active file を作成します。`log_rotation` は `none`、`hourly`、`daily`、`weekly`、`monthly` を指定できます。`monthly` は calendar month 境界ではなく30日周期です。`log_max_size_bytes` が0以外なら size 到達時にも早期 rotation します。`log_retention` は残す archive 数です。foreground、daemon、LaunchAgent のいずれでも process の reopen / restart は不要です。operator または外部 rotator が active path を置換・削除した場合も、kVNC は次の write 前に検出して設定済み path を reopen します。古い file descriptor ではなく `scripts/kvncctl logs` または設定済み path を確認してください。

## Linux の desktop capture: graphical user service を使う

`caffeinate` は macOS の command であり、Linux では使用しません。desktop capture は root system service や headless の lingering user service ではなく、sign-in 済み graphical user の session 内で動かします。systemd を使う distribution では、次の user unit を出発点にできます。path は置き換え、先に `~/.config/kvnc/config.toml` を設定してください。process lifecycle は systemd が管理するため、`daemon = false` のままにします。

```ini
# ~/.config/systemd/user/kvnc.service
[Unit]
Description=kVNC desktop server
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/systemd-inhibit --what=idle:sleep --mode=block --why=kVNC /home/YOUR_USER/path/to/kvnc/bin/kvnc
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
```

同じ graphical session 内の terminal で作成・有効化します。

```sh
mkdir -p ~/.config/systemd/user
systemctl --user daemon-reload
systemctl --user enable --now kvnc.service
systemctl --user status kvnc.service
```

`systemd-inhibit` は kVNC 実行中に logind が idle / sleep action を行わないよう要求しますが、desktop environment の display blanking、screen lock、Wayland portal policy は変更しません。X11 では graphical session と同じ `DISPLAY` / X authority を取得できる必要があります。Wayland は compositor と screen-capture portal が user service からの capture を許可するか決めるため、まず `--source demo` を実行し、その後に実 desktop session の smoke test を行います。distribution または desktop environment が `graphical-session.target` を提供しない場合は、linger を有効にしたり system service に移したりせず、その graphical-session の startup 機能を使ってください。

### Linux で client 接続時に画面を wake する場合

display を継続点灯させず、desktop VNC client の接続成功時に wake を要求するには、graphical user の設定へ次を追加します。

```toml
wake_display_on_connect = true
```

kVNC は active session type を選択し、keyboard / pointer input を偽装せず、実行した処理を log に残します。

- **X11:** kVNC process の `DISPLAY` / X authority を使い、`xset s reset`、続いて `xset dpms force on` を実行します。D-Bus service が使える場合は、その後に session-idle inhibition を5秒間保持します。`xset` utility（多くの distribution では `x11-xserver-utils` package）を導入し、同じ graphical session から kVNC を起動してください。通常の idle timer を reset するだけであり、以後の DPMS、lock、power policy を無効化するものではありません。
- **Wayland:** kVNC は graphical session の D-Bus に接続し、標準の `org.freedesktop.ScreenSaver.Inhibit` request を5秒間だけ保持してから解除します。対応 compositor では dimming や software screen saver 等の session idle action を抑止します。Wayland には、通常の client が物理的に消灯した monitor を強制点灯させる共通 API は意図的に存在しません。compositor が local user activity を必要とする場合があります。D-Bus service がない場合、kVNC は synthetic input へfallbackせず理由をlogに残します。

いずれも lock 済み session の解除や display manager の保護を bypass しません。capture backend がすでに全 monitor を失っている場合は、local で画面を wake してから kVNC を再起動してください。この設定は完全な RFB handshake（RA2ne 設定時は認証成功を含む）の後だけに実行し、raw TCP probe では実行しません。

停止・削除は次のとおりです。

```sh
systemctl --user disable --now kvnc.service
rm ~/.config/systemd/user/kvnc.service
systemctl --user daemon-reload
```

## macOS の desktop capture: LaunchAgent を使う

`--source desktop` は root LaunchDaemon や `sudo` ではなく、sign-in 中 user の **LaunchAgent** として起動してください。Screen Recording permission は sign-in 中 desktop user の process identity に付与されるため、system daemon は利用できません。

`~/Library/LaunchAgents/com.kazunari-kamata.kvnc.plist` を作成します。以下の path と trusted listener address は環境に合わせて置換してください。

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.kazunari-kamata.kvnc</string>
  <key>ProgramArguments</key><array>
    <string>/Users/YOUR_USER/path/to/kvnc/scripts/kvnc-launch-macos.sh</string>
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

repository の wrapper は最初に standalone の `/usr/bin/caffeinate -u -t 5` を実行し、終了を待ってから `/usr/bin/caffeinate -i bin/kvnc` を実行します。最初の5秒間の assertion は、capture backend が monitor を列挙する**前**に、すでに消灯中の display を wake します。これがないと macOS が active monitor なしと報告し、kVNC は desktop capture を開始できません。長時間の `-i` assertion は idle **system** sleep だけを抑止し、通常の display sleep は意図的に抑止しません。`-d` は意図的に使わないため、System Settings の通常の display sleep timer は有効なままです。

user configuration の `~/.config/kvnc/config.toml` が引数なしで読まれます。`WorkingDirectory` は configuration を選びません。LaunchAgent を使う場合は config の `daemon` を有効にしないでください。launchd が restart と process lifecycle を管理します。**System Settings → Privacy & Security → Screen Recording** では standalone の `kvnc` binary を許可してください（Terminal だけの許可では LaunchAgent には不十分です）。remote input を有効にする場合は **Accessibility** でも許可し、いずれかの permission を変更した場合は agent を再起動してください。local で rebuild / 置換した CLI binary は Accessibility の再許可が必要になる場合があります。build または `--update` 後は、log に `local keyboard and pointer input injection enabled` があることを確認してください。代わりに view-only mode が記録される場合は、Accessibility で現在の kVNC 起動 context を再度許可し、agent を再起動します。input permission がない場合も kVNC は view-only mode で待受を継続し、理由を log に記録します。`KeepAlive` は kill された wrapper を再起動するため、`caffeinate` を直接 kill せず、`scripts/kvncctl stop` または `launchctl bootout "gui/$(id -u)/com.kazunari-kamata.kvnc"` で job を停止します。start / status / restart と development instance は[起動・停止・再起動と開発時の運用](control.ja.md)を参照してください。

### 画面消灯を許可する場合

上記の標準 wrapper が省電力 profile です。macOS は System Settings で設定した通常の display sleep timer に従います。screen saver や `pmset` の設定は変更しません。local panel を常時点灯する明確な要件がない限り、wrapper に継続的な `-d` や `-u` を追加しないでください。この profile の目的に反します。`pmset displaysleepnow` を LaunchAgent に埋め込むと、active monitor なしの capture 状態を強制するため、使用しません。

### VNC client 接続時に画面を wake する場合

省電力 profile を使いつつ、現在は別 SSH session で `caffeinate -u -t 5` を実行して Mac を wake している場合は、代わりに次を有効にします。

```toml
# ~/.config/kvnc/config.toml
wake_display_on_connect = true
```

macOS かつ `source = "desktop"` の場合、kVNC は VNC client が RFB handshake を完了した後に `/usr/bin/caffeinate -u -t 5` を実行し、その5秒 command の完了を client の framebuffer request 処理前に待ちます。その後、その認証済み client の接続中だけ別の `caffeinate -d` assertion を保持します。client が切断すると assertion は解除されるため、VNC client がいない間は通常の macOS display-sleep timer が有効です。これは従来の SSH からの手動wakeと同じ順序です。RA2ne authentication を設定している場合は username/password の認証成功後であり、単なる TCP 接続では実行しません。接続成功により local screen が wake するため既定では無効です。trusted client だけに listener を公開してください。credential は記録せず、wake、assertion の取得・解除を log に残します。

この option を使う場合も、LaunchAgent command は `scripts/kvnc-launch-macos.sh` のままにします。これは restart 時に display sleep から回復するための一回だけの起動時 wake であり、display を点灯し続けるものではありません。接続時の setting は別の、認証後の wake です。display には active かつ unlock 済みの GUI session が必要です。macOS login window、FileVault、Screen Recording permission、物理的に切断された monitor を bypass するものではありません。Windows / Linux は上記および Windows guide で示した platform-native 処理を使います。

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

同じ user の GUI login session が有効であることが必要です。desktop session または monitor がない場合、`--source desktop` は framebuffer を取得できません。transport の切り分けには `--source demo` を使用してください。上記 LaunchAgent の例は start ごとに消灯中 display を wake し、その後は通常の display sleep を許可します。
