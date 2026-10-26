# SIGNING

`readit-preview.jks` signs preview builds (debug APKs) so each preview installs as an
update over the last one. Its password is public (`readit-preview`), so it proves nothing
about who built the app. **Never use it for a Play Store release.**

## RELEASE KEY (KEEP PRIVATE)

1. Create a key once and keep it backed up offline:
   `keytool -genkeypair -v -keystore readit-release.jks -alias readit -keyalg RSA -keysize 4096 -validity 10000`
2. In GitHub: Settings > Secrets and variables > Actions > New repository secret. Add:
   - `READIT_KEYSTORE_B64`: the keystore in base64 (`base64 -w0 readit-release.jks`, or on Windows
     `[Convert]::ToBase64String([IO.File]::ReadAllBytes("readit-release.jks"))` in PowerShell)
   - `READIT_KEYSTORE_PASSWORD`, `READIT_KEY_ALIAS` (for example `readit`), `READIT_KEY_PASSWORD`
3. The Android workflow then signs the release APK and App Bundle with that key.
