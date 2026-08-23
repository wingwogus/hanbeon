# Arduino CLI sidecars

Pinned Arduino CLI 1.5.1 binaries for Tauri `bundle.externalBin`.

Tauri selects the file whose suffix matches the current target triple:

- `arduino-cli-aarch64-apple-darwin`
- `arduino-cli-x86_64-apple-darwin`
- `arduino-cli-x86_64-pc-windows-msvc.exe`

These files are copied from the official v1.5.1 GitHub release archives. SHA-256
hashes live in `../resources/firmware/manifest.json`. Runtime code must not
download a replacement CLI.
