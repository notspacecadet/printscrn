# Stillshot

Stillshot replaces Snipping Tool, Lightshot, and the macOS screenshot overlay. Press the shortcut and the screen freezes at that instant, including menus, tooltips, and the current video frame. Drag a rectangle. When you let go, the PNG is already on the clipboard.

## Install

### Windows 10 or 11

1. Download `Stillshot-Setup.exe` from the latest build (GitHub Actions artifact `stillshot-windows`, or the file a release attaches).
2. Run it. It installs for your user only, under `%LOCALAPPDATA%\Stillshot`, and does not ask for an administrator password.
3. Windows SmartScreen will warn that the app is unsigned. Choose **More info**, then **Run anyway**.
4. Stillshot appears in the system tray. It also starts when you sign in.

If Print Screen still opens Snipping Tool, turn off **Settings → Accessibility → Keyboard → Use the Print screen key to open screen snipping**. Stillshot also installs a low-level hook so it can take Print Screen itself. If that hook cannot be installed, pick another shortcut in Settings.

A portable `stillshot.exe` is produced next to the installer. You can run that without installing. Close it from the tray menu.

### macOS 12 or later

1. Download `Stillshot.dmg` (GitHub Actions artifact `stillshot-macos`).
2. Open the disk image and drag **Stillshot** to **Applications**.
3. The app is unsigned. The first time you open it, macOS will say it cannot be verified.
   - Control-click the app in Finder, choose **Open**, then **Open** again.
   - Or, after one blocked launch: **System Settings → Privacy & Security**, scroll to the Stillshot message, and click **Open Anyway**.
   - Terminal alternative: `xattr -dr com.apple.quarantine /Applications/Stillshot.app`
4. Stillshot sits in the menu bar (no Dock icon). The first launch asks for **Screen Recording**.
   - Click **Grant access** and allow Stillshot in the system prompt.
   - If the frozen image is black or capture fails, quit Stillshot and open it again. macOS only applies that permission to new processes.
   - You can also open **System Settings → Privacy & Security → Screen Recording** yourself. There is a button for that in Stillshot’s settings.

The disk image contains a universal binary (Apple silicon and Intel).

## Use it

| Action | Result |
| --- | --- |
| Shortcut | Freeze every display as it is right now |
| Drag | Select a region. Releasing the mouse copies the PNG immediately |
| Enter | Copy the selection, including annotations, and close |
| Double-click | Copy the whole display under the pointer |
| Esc | Cancel the current mark, or close the overlay |
| Ctrl/Cmd+C | Copy the selection (with annotations) and close |
| Ctrl/Cmd+Z | Undo |
| Ctrl/Cmd+Shift+Z or Ctrl/Cmd+Y | Redo |
| Ctrl/Cmd+S | Save a copy somewhere else |

After the region is copied, a toolbar appears on the selection. Drawing does not get in the way of the first copy: if you only drag and release, you are done. Each finished mark (pen, shape, text, blur) updates the clipboard and, if saving is on, the same file.

Toolbar tools: pen, highlighter, line, arrow, rectangle, ellipse, text, blur, pixelate, eyedropper, colors, stroke width, undo, redo, Copy, Save, Close. Hold Shift while drawing a line, arrow, rectangle, or ellipse to constrain it.

Default shortcuts:

- Windows: **Print Screen**
- Mac: **⌘⇧2** (Command-Shift-2, which is not used by the system screenshot keys)

Change the shortcut from the tray menu → **Settings…**.

Copies are also written to `~/Pictures/Screenshots` (on Windows, your Pictures folder, then `Screenshots`). Turn that off in Settings. Files are named like `Screenshot 2026-11-10 at 23.45.01.png`.

## What gets put on the clipboard

The bitmap is encoded for the programs you paste into (Slack, WhatsApp Web, Chrome, Claude, Gmail, Paint, Preview):

- **Windows:** a PNG clipboard format (`RegisterClipboardFormat("PNG")`), a classic `CF_DIB` (bottom-up 32-bit BGRA), and a `CF_DIBV5` with alpha masks. PNG is written first because browsers and Electron apps prefer it. Paint and older Win32 apps ask for `CF_DIB`.
- **macOS:** `NSPasteboard` gets both `public.png` and `public.tiff`.
- **Linux** (for development): `image/png` on the X11 clipboard. The app has to keep running, which it does, in the tray.

## Why this stack

Stillshot is a native Rust app: [eframe]/[egui] for the overlay, [xcap] for capture, and a small amount of platform clipboard code.

A WebView (Tauri or Electron) is the wrong trade for this tool. The frozen frame is a full-monitor RGBA buffer, often tens of megabytes once you include a Retina display. Shipping that through a browser and decoding it as a PNG blows the time budget before the overlay is even visible. Here the pixels stay in process, the overlay window is a borderless fullscreen viewport per monitor, and the image is uploaded as a GPU texture. Capture itself uses DXGI on Windows and Core Graphics on macOS, which is what the system screenshot tools use.

HiDPI: each display is captured at its physical pixel size. A selection that lives on one screen is cropped 1:1 out of that bitmap, so a Retina snip is not a blurry downsample. A selection that crosses two screens uses the scale of the screen that contains most of it.

## Build it yourself

You need a current stable Rust (1.95 or newer; eframe 0.37 sets that floor).

### Windows

From a PowerShell prompt in this folder, with [NSIS](https://nsis.sourceforge.io/) installed (`choco install nsis` if you use Chocolatey):

```powershell
.\scripts\build-windows.ps1
```

Outputs:

- `dist\stillshot.exe`
- `dist\Stillshot-Setup.exe`

### macOS

From Terminal, with the Xcode command-line tools installed:

```bash
bash scripts/build-macos.sh
```

That builds both `aarch64-apple-darwin` and `x86_64-apple-darwin`, merges them with `lipo`, and wraps the result in `dist/Stillshot.dmg`.

### Tests

```bash
cargo test --lib
```

Those tests do not need a display. They cover region math (including a 2x HiDPI crop), PNG/TIFF/DIB byte layouts, the annotation rasterizer, undo/redo, shortcut parsing, and screenshot file names.

### GitHub Actions

`.github/workflows/build.yml` runs the tests on Linux and uploads:

- `stillshot-windows`: `stillshot.exe` and `Stillshot-Setup.exe`
- `stillshot-macos`: `Stillshot.dmg`

This environment is Linux, so it cannot cross-link the Windows or macOS GUI binaries. The workflow is the build that produces the installers.

## What was verified here

On this Linux machine, with a 1920×1200 X11 display:

- `cargo test --lib`: 31 tests passed.
- `stillshot --self-check` captured the display and round-tripped the image through the clipboard. The center pixel matched. Capture was about 80 ms. Encoding the full 1920×1200 frame for the clipboard is much slower in a debug build than a normal region snip; release builds use the same fast PNG encoder.
- `stillshot --capture` opened a borderless 1920×1200 overlay on the display. The frozen frame filled the screen, with the hint “Drag a region — it copies when you let go”. In a debug build the first paint landed about 190 ms after the capture started, and about 110 ms of that was showing the new window. A second capture, with the window already open, was about 150 ms in the same debug build. Release mode and a native DXGI or Core Graphics capture are both faster than X11 `GetImage` on this machine. The design target is under ~150 ms on Windows and macOS; that was not measured on those operating systems here.
- Clipboard formats are tested as bytes everywhere: PNG and TIFF round-trip, and the DIB headers are the classic 40-byte `BITMAPINFOHEADER` plus a 124-byte `BITMAPV5HEADER` with alpha masks. The Windows `OpenClipboard` path and the macOS `NSPasteboard` path are compiled only on those OSes, so they were not executed here.
- Dragging a region with `xdotool` did not reach the app. winit on X11 ignores pointer events marked emulated, which is how this VNC server reports synthetic clicks. A real mouse is what the overlay is built for. Keyboard shortcuts from `xdotool` had the same limitation.

Not verified here: the Windows installer, the macOS disk image, Screen Recording permission UI on a Mac, Print Screen vs Snipping Tool on Windows 11, and pasting into Slack, Chrome, or Preview on those systems. The CI workflow is what builds those artifacts.

## License

MIT OR Apache-2.0. See `LICENSE`.

The annotation font is [Inter](https://github.com/rsms/inter), licensed under the SIL Open Font License 1.1. See `assets/Inter-OFL.txt`.
