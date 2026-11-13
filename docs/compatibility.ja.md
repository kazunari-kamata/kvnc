# 対応環境

English: [compatibility.en.md](compatibility.en.md)

| Platform | Desktop capture | Clipboard | CI verification |
| --- | --- | --- | --- |
| macOS arm64 | Screen Recording permission を許可すると対応 | 対応 | `macos-15` で native build / test |
| macOS x86_64 | Screen Recording permission を許可すると対応 | 対応 | `macos-15-intel` で native build / test |
| Windows x86_64 | interactive desktop session で対応 | 対応 | `windows-latest` で native build / test |
| Linux x86_64 / X11 | interactive desktop session で対応 | 対応 | `ubuntu-latest` で native build / test |
| Linux / Wayland | environment / portal に依存 | environment に依存 | Ubuntu で native build / test、実機 smoke test が必要 |

`--source demo` は desktop を取得しないため、RFB protocol の動作を portable に検証できます。CI build は screen-capture permission や全ての実 desktop session を作成できません。`--source desktop` に依存する前に deployment OS で smoke test を実行してください。

## macOS の設定

kVNC は `sudo` を使わず、sign-in 中の desktop user として起動します。**System Settings → Privacy & Security → Screen Recording** で起動元 application を許可し、起動元を完全に再起動してください。まず `--source demo --debug --log-file kvnc.log` で、Screen Recording permission と切り離して RFB 接続を確認できます。

## Windows の設定

kVNC は Windows service ではなく、sign-in 済み interactive user session で起動します。lock screen と UAC secure desktop は保護された別 desktop のため、通常 user session の capture / input 注入の対象外です。Git clone、`--init`、`--update-config`、`--update`、logon task、power policy の手順は [Windows ガイド](windows.ja.md) を参照してください。

## Linux の設定

kVNC は root system service や headless の lingering user service ではなく、sign-in 済み graphical user session で起動します。X11 では graphical session の display と X authority が必要です。Wayland の capture / clipboard は compositor と screen-capture portal に依存するため、対象 distribution で `--source demo`、続けて実 desktop を確認してください。`systemd --user` service と任意の `systemd-inhibit` wrapper は[バックグラウンド運用](daemon.ja.md)を参照し、display blanking / screen lock は kVNC ではなく desktop environment 側で設定します。
