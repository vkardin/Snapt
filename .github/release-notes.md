## Download

| Platform | File |
| --- | --- |
| Windows 10/11 | `snapt_*_x64-setup.exe` (installer) or `snapt_*_x64_en-US.msi` |
| macOS, Apple Silicon (M1 and newer) | `Snapt_*_aarch64.dmg` |
| macOS, Intel | `Snapt_*_x64.dmg` |
| Linux, Debian and Ubuntu | `snapt_*_amd64.deb` |
| Linux, other distributions | `snapt_*_x86_64.AppImage` |

`SHA256SUMS.txt` lists the checksums of all files.

## First launch

Snapt is not code-signed yet, so the system warns you the first time you open it.

- **Windows.** SmartScreen shows "Windows protected your PC". Click **More info**, then **Run anyway**.
- **macOS.** Open the `.dmg` and drag Snapt to Applications. On the first launch macOS says it cannot
  verify the developer. Open **System Settings → Privacy & Security**, scroll down and click
  **Open Anyway**. If macOS says the app is damaged, run
  `xattr -dr com.apple.quarantine /Applications/Snapt.app` in Terminal. On the first capture,
  allow Screen Recording in the same settings page and restart Snapt.
- **Linux.** Install the `.deb` with `sudo apt install ./snapt_*_amd64.deb`, or make the AppImage
  executable with `chmod +x snapt_*.AppImage` and run it. On GNOME, install the AppIndicator
  extension to see the tray icon. On Wayland, bind `snapt --capture` to a key in the keyboard
  settings, since apps can't register global shortcuts there.

Snapt starts in the tray (menu bar on macOS). Press **PrtSc** (**Cmd+Shift+9** on macOS) to take a
screenshot, select an area, draw on it with the panel next to it, then press `Ctrl+C` to copy or
`Ctrl+S` to save. The shortcut, folder and format are in the tray menu under **Settings**.
