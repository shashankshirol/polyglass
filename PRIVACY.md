# Privacy

Plain-language description of what Polyglass does with your data. (A Swedish version will ship
with 1.0, SV-08.)

## On-device mode (the default)

- Polyglass captures **only the area under the lens**, in memory.
- Text is recognized and translated **on your computer**. Nothing is sent anywhere.
- Screen images are **never saved to disk**. The only exception is a developer option
  (`--debug-dump-frames`) that shows a red banner the whole time it is on.
- Translations are cached **in memory only** and forgotten when you quit.
- Network traffic happens only when **you** download a translation model, or if you turn on
  update checks. Model downloads come only from addresses listed in the app's built-in
  manifest and are checked against a fingerprint (SHA-256) before use.
- The translation models are downloaded from **GitHub**: the releases of the Polyglass
  repository (`github.com`), which hands the file over to GitHub's download servers. Like any
  download, GitHub sees your IP address and the files requested
  ([GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement)).
  Polyglass sends no account, identifier or screen content with it, over HTTPS only, and only
  when you choose Download in the welcome window or the model manager. You can also build and install the models yourself without contacting
  GitHub (README, "First run").

## Online translation (opt-in)

If you choose an online service (LibreTranslate, DeepL or Google):

- Polyglass shows a consent dialog first, naming the provider and linking its privacy policy.
- Only the **recognized text and the language pair** are sent — never screen images.
- The lens always shows a cloud icon and the provider name while an online service is active.
- API keys are stored in your operating system's keychain, never in plain files.
- Settings lists every network request made in the current session (time, host, size — no
  content).

## What Polyglass never does

- No analytics, telemetry or tracking.
- No automatic crash uploads. Crash reports are saved locally; you decide whether to attach
  one to a bug report.
- Logs at the default level contain no recognized or translated text.
