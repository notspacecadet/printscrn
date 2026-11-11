# Stillshot

Press the shortcut and Stillshot freezes the screen at that instant, including menus, tooltips, and the current video frame. Drag a rectangle. When you let go, the PNG is already on the clipboard.

## Download

Windows 10 or 11, and macOS 12 or later. These links always point at the latest release.

|  |  |
| --- | --- |
| Windows installer | [Stillshot-Setup.exe](https://github.com/notspacecadet/printscrn/releases/latest/download/Stillshot-Setup.exe) |
| Windows portable app | [stillshot.exe](https://github.com/notspacecadet/printscrn/releases/latest/download/stillshot.exe) |
| macOS disk image | [Stillshot.dmg](https://github.com/notspacecadet/printscrn/releases/latest/download/Stillshot.dmg) |

All releases are listed on the [releases page](https://github.com/notspacecadet/printscrn/releases/latest).

**Windows installer.** Run `Stillshot-Setup.exe`. It installs for your user only, under `%LOCALAPPDATA%\Stillshot`, and does not ask for an administrator password. Stillshot appears in the system tray and starts when you sign in.

**Windows portable.** Run `stillshot.exe` without installing. Close it from the tray menu.

**macOS.** Open `Stillshot.dmg` and drag Stillshot to Applications. It sits in the menu bar and does not show a Dock icon. The disk image is a universal app (Apple silicon and Intel).

Read [First launch](#first-launch) before the first open. The builds are unsigned.

## How to use

1. Press the shortcut. Windows uses **Print Screen**. Mac uses **Command-Shift-2**.
2. The screen freezes as it is right now. Drag a rectangle.
3. Releasing the mouse copies that region to the clipboard. Paste it into Slack, Chrome, Paint, Preview, or anywhere else that accepts an image.
4. A toolbar appears on the selection if you want to mark it up. Each finished mark updates the clipboard. If you only drag and release, you are done.
5. Press **Enter** or **Ctrl+C** / **Command-C** to copy the selection, including annotations, and close. **Esc** or a right-click closes the overlay immediately. Double-click copies the whole display under the pointer. The overlay also closes itself after 60 seconds.

Stillshot does not freeze the screen when it starts, and it ignores the shortcut while the overlay is already open and for half a second after it closes. **Ctrl+Alt+Q** quits Stillshot and removes the overlay.

Copies are also written to `~/Pictures/Screenshots` (on Windows, your Pictures folder, then `Screenshots`). Files are named like `Screenshot 2026-11-10 at 23.45.01.png`. Turn saving off in Settings.

Change the shortcut from the tray menu on Windows, or the menu bar on Mac: **Settings…**.

Toolbar tools: pen, highlighter, line, arrow, rectangle, ellipse, text, blur, pixelate, eyedropper, colors, stroke width, undo, redo, Copy, Save, and Close. Hold Shift while drawing a line, arrow, rectangle, or ellipse to keep it straight or square.

## Features

- Freezes every display at the moment you press the shortcut, so menus, tooltips, and video frames stay in the picture.
- Copies a PNG as soon as you release the drag.
- Annotate after that first copy. Drawing does not get in the way if you only wanted the snip.
- A selection on one screen is cropped 1:1 from that display’s pixels, including Retina and other high-DPI screens.
- Saves the same image under Pictures/Screenshots, and you can turn that off.
- Lives in the system tray (Windows) or the menu bar (Mac) and starts when you sign in.
- The Windows installer is per-user and does not need an administrator password.

## Keyboard shortcuts

| Action | Shortcut |
| --- | --- |
| Freeze the screen | Print Screen on Windows, Command-Shift-2 on Mac |
| Copy the selection, including annotations, and close | Enter, or Ctrl+C / Command-C |
| Copy the whole display under the pointer | Double-click |
| Close the overlay immediately | Esc, or right-click |
| Undo | Ctrl+Z / Command-Z |
| Redo | Ctrl+Shift+Z / Command-Shift-Z, or Ctrl+Y / Command-Y |
| Save a copy somewhere else | Ctrl+S / Command-S |
| Quit Stillshot and close the overlay | Ctrl+Alt+Q |
| Constrain a line, arrow, rectangle, or ellipse | Hold Shift while drawing |

## First launch

The downloads are unsigned.

**Windows SmartScreen.** When Windows says it protected your PC, choose **More info**, then **Run anyway**.

If Print Screen still opens Snipping Tool, turn off **Settings → Accessibility → Keyboard → Use the Print screen key to open screen snipping**. Stillshot also installs a hook so it can take Print Screen itself. If that hook cannot be installed, pick another shortcut in Settings.

**macOS.** The first open says the app cannot be verified. Right-click Stillshot in Finder, choose **Open**, then **Open** again. After a blocked launch you can also use **System Settings → Privacy & Security** and click **Open Anyway**.

The first launch asks for **Screen Recording**. Allow it. If the frozen image is black or capture fails, quit Stillshot and open it again. macOS applies that permission only to new processes. Stillshot’s settings include a button that opens **System Settings → Privacy & Security → Screen Recording**.

## Uninstall

**Windows.** Open the Start menu shortcut **Stillshot → Uninstall**, or run `%LOCALAPPDATA%\Stillshot\Uninstall.exe`. That removes the app, the Start menu shortcuts, and the sign-in startup entry. It does not ask for an administrator password. Screenshot files already saved under Pictures/Screenshots stay where they are.

**Portable Windows build.** Quit from the tray menu and delete `stillshot.exe`.

**macOS.** Quit from the menu bar, then drag `Stillshot.app` from Applications to the Trash. To revoke Screen Recording, open **System Settings → Privacy & Security → Screen Recording** and remove Stillshot.

## FAQ

**If the frozen screen gets stuck.** Press **Esc**, right-click, or wait up to 60 seconds. The overlay closes on its own. **Ctrl+Alt+Q** quits Stillshot. **Ctrl+Alt+Del** still opens the Windows security screen, and Task Manager can cover the overlay. Quit is also in the tray menu or the Mac menu bar. A log is written to `%LOCALAPPDATA%\Stillshot\stillshot.log` on Windows. Stillshot does not capture when it launches, and a second launch does not start another copy.

**Paste works in a browser but not in Paint, or the other way around.** The copy includes the formats those apps ask for: PNG for browsers and chat apps, and the classic bitmap formats for Paint and other Windows apps. On Mac the pasteboard gets both PNG and TIFF.

**The shortcut does nothing.** Confirm Stillshot is running (tray icon or menu bar). Open Settings and check the shortcut. On Windows, turn off the Snipping Tool Print Screen setting described above.

**The Mac capture is black.** Screen Recording was granted after Stillshot had already started. Quit and open it again.

**Where are the files?** `~/Pictures/Screenshots`, unless you turned saving off or chose another folder in Settings.

**Does it need an administrator password?** No. The Windows installer is per-user. On Mac you only drag the app to Applications.

## Build from source

You need Rust 1.95 or newer. Windows and macOS builds, tests, and the release workflow are documented in [docs/building.md](docs/building.md).

## License

MIT OR Apache-2.0. See [LICENSE](LICENSE).

The annotation font is [Inter](https://github.com/rsms/inter), licensed under the SIL Open Font License 1.1. See [assets/Inter-OFL.txt](assets/Inter-OFL.txt).
