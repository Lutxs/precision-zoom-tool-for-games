# Precision Zoom Tool

A lightweight Windows magnifier overlay that follows the cursor while you hold a key, with crosshair/PNG reticle support, circular scope mode, negative contrast mode and a tray-based settings window. Written in Rust.

## Preview

<img width="296" height="515" alt="image" src="https://github.com/user-attachments/assets/06896cc4-4a75-49ca-bdf3-dd28f55cedca" />

### Disabled:
<img width="1903" height="1396" alt="image" src="https://github.com/user-attachments/assets/c0a3fd1f-7b6b-4e75-a224-3b7c0f1a9308" />

### Enabled:
<img width="1505" height="1222" alt="image" src="https://github.com/user-attachments/assets/9ac55dfc-e62b-49a5-90f1-0e2ca6b48fd1" />

## Features

- Zooms the area around the cursor using the Windows Magnification API
- Built-in crosshair or your own PNG reticle (per-pixel alpha, so soft edges look right)
- Circular scope mode with an anti-aliased border
- Negative (inverted colors) mode
- Adjustable zoom, box size, opacity, border width/color, reticle size/line width/color and refresh interval
- X/Y offset controls for the reticle
- Configurable hotkeys (zoom key, toggle modifier, zoom in/out)
- Settings apply instantly, with no restart needed
- Input validation with clear messages; invalid values are never saved
- Optional "hide overlay from screen capture"
- Tray icon that shows on/off state, with Enabled / Settings / Exit menu
- Single instance: launching it again opens the running copy's settings
- Per-monitor DPI aware settings window
- Settings stored per value in the current user's registry; settings from the old C++ version are migrated automatically

## Controls (defaults)

| Action | Default | Configurable |
|---|---|---|
| Zoom while held | Right mouse button | Yes |
| Toggle the tool on/off | Hold Left Shift + Right mouse button | Yes (modifier) |
| Zoom in / out while zoomed | `=` / `-` (tap = one step, hold to repeat) | Yes |
| Settings | Double-click the tray icon, or right-click → Settings | |
| Exit | Tray icon → Exit | |

Zoom changed with the hotkeys is remembered: it's saved when you release the zoom key.

## Settings

| Setting | Range |
|---|---|
| Zoom factor | 1.1 – 16 |
| Box size (px) | 64 – 1600 |
| Opacity (%) | 10 – 100 |
| Refresh interval (ms) | 1 – 100 |
| Border (px) and color | 0 – 50 |
| Circular scope, Negative mode | on/off |
| Show reticle | on/off |
| Reticle size (px) | 4 – 1600 |
| Reticle line width (px) and color | 1 – 25 |
| X / Y offset (px) | ± half the box size |
| Reticle image | PNG file, or the built-in cross |
| Hotkeys | Hold to zoom, toggle modifier, zoom in, zoom out |
| Hide overlay from screen capture | on/off (see Notes) |
| Beep when toggled | on/off |

## Download

Prebuilt executables are attached to each [GitHub release](https://github.com/Lutxs/precision-zoom-tool-for-games/releases). Every push also builds one as a CI artifact.

## Building

Requirements: Windows 10 or 11, [Rust](https://rustup.rs) (stable, MSVC toolchain) and the Visual Studio Build Tools with the Windows SDK (needed by the MSVC linker and the resource compiler).

```powershell
.\build.ps1          # fmt check, clippy, tests, then a release build into .\dist
.\build.ps1 -Fast    # release build only
```

Or with Cargo directly:

```powershell
cargo build --release   # target\release\precision-zoom.exe
cargo test
```

Run `precision-zoom.exe`. Pass `--settings` to open the settings window on startup.

To publish a release, push a tag such as `v0.2.0`. CI builds the executable and attaches it to a GitHub release.

## How it works

The app creates:
- a hidden controller window that owns the tray icon and does all the work on the UI thread,
- a magnifier host window containing the Windows magnifier control,
- a click-through overlay window that draws the border and reticle with per-pixel alpha (rendered with [tiny-skia](https://github.com/linebender/tiny-skia)),
- the settings window.

A small background thread only acts as a high-resolution timer. On each tick it posts a message to the UI thread, which polls the hotkeys, moves both windows in one atomic step and refreshes the magnified image. Windows are never touched from the background thread. Tick messages are also handled while menus or dialogs are open, so zoom keeps working then.

Source layout:

| File | Purpose |
|---|---|
| `src/app.rs` | Controller, magnifier/overlay windows, tray icon, per-tick logic |
| `src/settings.rs` | Settings window |
| `src/config.rs` | Settings model, validation, registry storage, legacy migration |
| `src/input.rs` | Hotkey state machine (toggle, auto-repeat) |
| `src/geometry.rs` | Zoom stepping and capture-rectangle math |
| `src/overlay.rs` | Border/reticle rendering and layered-window upload |
| `src/ticker.rs` | High-resolution tick thread |
| `src/keys.rs` | Bindable keys |
| `build.rs` | Generates the app icons; embeds icons, manifest and version info |

## Registry storage

Settings are stored as individual values under:

`HKCU\Software\PrecisionZoomTool`

If a `Settings` binary value from the C++ version is found, it is converted once and then removed.

## Notes

- IMPORTANT: This tool uses topmost layered windows and screen magnification, so some games or strict anti-cheat systems may restrict it. It does not inject into other processes, install hooks or read/modify game memory. Keys are read by polling.
- **Hide overlay from screen capture** keeps the magnifier and reticle out of screenshots and recordings (`SetWindowDisplayAffinity`). Modern anti-cheats often take silent screenshots to look for overlays, and setting this flag effectively says "don't look at this window", which may be treated as suspicious. It is off by default.
- If a PNG reticle can't be loaded (moved, deleted, not a PNG), the app falls back to the built-in cross.
- Intended for desktop use and for games where overlays are permitted.

## Changes from the C++ version

- **Fixed:** crosshair "ghosting", where the reticle got magnified along with the screen. The magnifier ignores its exclusion list for windows that are hidden when the list is set. The list is now set right after the windows are shown and before the first paint, and the zoom transform and capture area are also set before anything becomes visible. The screen-capture workaround is no longer needed and is now an opt-in setting.
- **Fixed:** a zoom factor of 0 or an empty field divided by zero. A box size or opacity of 0 broke the overlay. All input is now validated and clamped.
- **Fixed:** settings saved as a raw struct dump broke whenever the layout changed. They are now stored per value, with migration.
- **Fixed:** the zoom transform and color effect were re-sent every 10 ms. They are now set only when they change.
- **Fixed:** `+`/`-` jumped about five steps per second regardless of a tap, and the result was lost on restart. Taps are now single steps with auto-repeat, and the result is saved.
- **Fixed:** windows were manipulated from the input thread. All window work now happens on the UI thread.
- **Fixed:** releasing the toggle modifier before the zoom key popped the magnifier up right after a toggle.
- **Fixed:** PNG paths with non-ASCII characters (ANSI code page) failed. Paths are now Unicode.
- **New:** app icon with on/off tray states, live settings (no relaunch), configurable hotkeys and colors, a DPI-aware settings window, single-instance handling, and the tray icon is restored when Explorer restarts.
- **New:** build script, `.gitignore`, and GitHub Actions CI that builds, tests and attaches the `.exe` to releases.

## Development Note

This project was initially generated with the assistance of AI and then reviewed and refined. The Rust rewrite was also written with AI assistance (Claude Code).

## License

MIT License

Copyright (c) 2026 Lutxs

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
