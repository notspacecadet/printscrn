# Build Stillshot from source

Stillshot is a native Rust app. The overlay is [eframe](https://github.com/emilk/egui) / egui, capture is [xcap](https://github.com/nashaofu/xcap), and a small amount of platform code writes the clipboard. Pixels stay in process: each display is captured at its physical size, the overlay is a borderless fullscreen window per monitor, and the image is uploaded as a GPU texture.

You need a current stable Rust (1.95 or newer; eframe 0.37 sets that floor).

## Windows

From PowerShell in this repo, with [NSIS](https://nsis.sourceforge.io/) installed (`choco install nsis` if you use Chocolatey):

```powershell
.\scripts\build-windows.ps1
```

Outputs:

- `dist\stillshot.exe`
- `dist\Stillshot-Setup.exe`

The installer script expects paths relative to the repo root. The build script invokes NSIS with `/NOCD`.

## macOS

From Terminal, with the Xcode command-line tools installed:

```bash
bash scripts/build-macos.sh
```

That builds both `aarch64-apple-darwin` and `x86_64-apple-darwin`, merges them with `lipo`, and wraps the result in `dist/Stillshot.dmg`.

## Tests

```bash
cargo test --lib
```

Those tests do not need a display. They cover region math (including a 2x HiDPI crop), PNG/TIFF/DIB byte layouts, the annotation rasterizer, undo/redo, shortcut parsing, and screenshot file names.

## GitHub Actions

[`.github/workflows/build.yml`](../.github/workflows/build.yml) runs on pushes to `main` and on `workflow_dispatch`. A push of a tag matching `v*` also publishes a GitHub Release with:

- `Stillshot-Setup.exe`
- `stillshot.exe`
- `Stillshot.dmg`

The macOS job runs on `macos-15-intel` and still produces a universal binary. Linux hosts cannot cross-link the Windows or macOS GUI binaries, so the workflow is what produces the installers.

## Clipboard formats

- **Windows:** a PNG clipboard format (`RegisterClipboardFormat("PNG")`), a classic `CF_DIB` (bottom-up 32-bit BGRA), and a `CF_DIBV5` with alpha masks. PNG is written first because browsers and Electron apps prefer it. Paint and older Win32 apps ask for `CF_DIB`.
- **macOS:** `NSPasteboard` gets both `public.png` and `public.tiff`.
- **Linux** (for development): `image/png` on the X11 clipboard. The process has to stay running, which it does, in the tray.
