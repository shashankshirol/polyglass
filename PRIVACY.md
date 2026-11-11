# Privacy

Plain-language description of what Polyglass does with your data.

## Everything runs on your device

Polyglass has no online translation services: version 1 translates only on your computer.

- Polyglass captures **only the area under the lens**, in memory. Freeze mode (❄,
  Ctrl+Alt+F) takes one picture of the monitor under the lens instead; it is kept in memory
  only and thrown away when you leave Freeze mode.
- Text is recognized and translated **on your computer**. Nothing is sent anywhere.
- Screen images are **never saved to disk**. The only exception is a developer option
  (`--debug-dump-frames`) that shows a red banner the whole time it is on.
- Translations are cached **in memory only** and forgotten when you quit.
- Network traffic happens only when **you** download a translation model. Polyglass never
  checks for updates. Model downloads come only from addresses listed in the app's built-in
  manifest and are checked against a fingerprint (SHA-256) before use.
- The translation models are downloaded from **GitHub**: the releases of the Polyglass
  repository (`github.com`), which hands the file over to GitHub's download servers. Like any
  download, GitHub sees your IP address and the files requested
  ([GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement)).
  Polyglass sends no account, identifier or screen content with it, over HTTPS only, and only
  when you choose Download in the welcome window or the model manager. You can also build and install the models yourself without contacting
  GitHub (README, "First run").
- On Windows the windows are drawn by Microsoft Edge WebView2, which has background services
  of its own. Polyglass turns them off (update and experiment checks, pings, metrics uploads,
  and the library that signs in with your Windows account). In testing, neither Polyglass nor
  WebView2 then opened any network connection. A WebView2 update could add new services, so
  this is re-checked when the runtime changes.

## What Polyglass never does

- No analytics, telemetry or tracking.
- No automatic crash uploads. If Polyglass crashes it saves a report in its own data folder
  (`crash-reports`, the newest 10 are kept; the tray's "Crash reports…" opens it). The report
  holds the version, OS and where the program stopped, not your screen's text. You decide
  whether to attach one to a bug report.
- Logs at the default level contain no recognized or translated text.
