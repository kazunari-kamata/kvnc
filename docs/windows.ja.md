# Windows ガイド

English: [windows.en.md](windows.en.md).

## clone、build、実行確認

Git、stable Rust **MSVC** toolchain、Windows SDK を含む Visual Studio Build
Tools を導入します。共有する desktop の sign-in 済み user で PowerShell を開き、clone と build を行います。

```powershell
git clone https://github.com/kazunari-kamata/kvnc.git
Set-Location kvnc
cargo install --path . --root . --force --locked
.\bin\kvnc.exe --version
.\bin\kvnc.exe --help
```

`bin\kvnc.exe` は native standalone executable です。Cargo は build / update 時だけ必要で、実行時には不要です。interactive desktop capture には管理者昇格や Windows service を使わず、共有対象の desktop / clipboard / keyboard / pointer と同じ sign-in user session から起動してください。

## 初期設定

comment 付きの per-user configuration を安全に作成します。Git checkout 内には作成しないため、Git の push 対象にはなりません。

```powershell
.\bin\kvnc.exe --init
.\bin\kvnc.exe --print-config
notepad "$env:APPDATA\kvnc\config.toml"
```

通常の path は `%APPDATA%\kvnc\config.toml` です。`--init` は既存 file を上書きしません。内容を確認したうえで明示的に置き換える場合だけ `--init --force` を使います。別 path を作る場合は `--config C:\path\to\kvnc.toml --init` を使います。

生成直後は loopback listener と input 無効という安全な設定です。`scale = 1.0` で native capture resolution を維持し、文字の鮮明さを優先します。server 起動前に必ず `listen` と `allow_input` を確認してください。log file は既定で `%APPDATA%\kvnc\kvnc.log` です。

`allow_input = false` の間、VNC client からの mouse / keyboard event は意図的に無視されます。trusted listener と trusted client だけで使う場合は、config の値を `true` に変更するか、初期生成時に明示的に opt-in します。

```powershell
.\bin\kvnc.exe --init --allow-input
```

以前の kVNC で生成した config に `scale = 0.5` がある場合は、`1.0` に変更して kVNC を再起動すると full-resolution capture になります。VNC client 側の「scale to fit」や viewport scaling も、鮮明さを確認する際は無効にしてください。

## 起動と確認

設定済み desktop server は引数なしで起動します。

```powershell
.\bin\kvnc.exe
```

desktop capture permission を使わず RFB / clipboard だけを確認する場合は、demo source を使います。

```powershell
.\bin\kvnc.exe --source demo --listen 127.0.0.1:5900 --debug --log-file .\kvnc.log
```

desktop monitor を選択する前に一覧を確認します。

```powershell
.\bin\kvnc.exe --list-monitors
.\bin\kvnc.exe --source desktop --monitor 0 --allow-input --listen 127.0.0.1:5900
```

Windows lock screen と UAC secure desktop は別の保護された desktop です。通常の user session からの capture / input 注入は期待しないでください。trusted private network の client を受ける場合は特定の private interface address に bind し、Windows Defender Firewall でも当該 trusted network だけを許可します。unencrypted RFB を public Internet に公開してはいけません。

## sign-in 時の起動と sleep の扱い

kVNC には macOS の `caffeinate` wrapper に相当する Windows 機能はなく、computer の power policy も変更しません。sign-in 時に再起動するには、**Task Scheduler** で次の内容の task を作成します。

1. **General** で desktop user を選び、**Run only when user is logged on** を選択します。**Run whether user is logged on or not** は選ばず、Windows service も使用しません。
2. その user の **At log on** trigger を追加します。
3. **Start a program** action の program に、`kvnc.exe` の absolute path を指定します。command-line argument は不要で、当該 user の `%APPDATA%\kvnc\config.toml` を読みます。
4. **Settings** では必要に応じて restart-on-failure を有効にします。task は interactive user session 内に留め、Session 0 へ移してはいけません。

task 作成後は sign-out / sign-in して、同じ user session から listener を確認します。

```powershell
Get-Process kvnc
Get-NetTCPConnection -LocalPort 5900 -State Listen
Get-Content "$env:APPDATA\kvnc\kvnc.log" -Tail 50
```

sleep、hibernation、lock-screen transition、display power timeout により、interactive desktop が capture / input に使えなくなることがあります。trusted remote access 用 host の **Settings → System → Power & battery → Screen and sleep** は AC / battery の両方で意図どおりか確認してください。kVNC のためだけに lock-screen や power policy を弱めてはいけません。locked / secure desktop は、この user-session server の対象外です。

## clone の保守

`--update-config` は既存 value / comment を保ったまま、未設定の top-level default だけを追加します。実行前の config は同じ directory に timestamp 付きで backup されます。

```powershell
.\bin\kvnc.exe --update-config
```

`--update`（`--upgrade` も同義）は clean な Git clone でのみ使用できます。`git pull --ff-only` を実行します。Windows では実行中の `kvnc.exe` を置き換えられないため、終了後に必要な rebuild command を表示します。

```powershell
.\bin\kvnc.exe --update
cargo install --path . --root . --force --locked
.\bin\kvnc.exe --version
```

GitHub Release artifact を download しただけの場合は、artifact を手動で置き換えてください。`--update` は意図的にそれを変更しません。
