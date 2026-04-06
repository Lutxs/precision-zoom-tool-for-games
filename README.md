# Precision Zoom Tool

A lightweight Windows desktop zoom overlay with optional crosshair support, PNG reticle loading, circular scope mode, negative contrast mode, and tray-based settings access.

## Preview

<img width="296" height="515" alt="image" src="https://github.com/user-attachments/assets/06896cc4-4a75-49ca-bdf3-dd28f55cedca" />

### Disabled:
<img width="1903" height="1396" alt="image" src="https://github.com/user-attachments/assets/c0a3fd1f-7b6b-4e75-a224-3b7c0f1a9308" />

### Enabled:
<img width="1505" height="1222" alt="image" src="https://github.com/user-attachments/assets/9ac55dfc-e62b-49a5-90f1-0e2ca6b48fd1" />


## Features

- Zooms the area around the cursor using the Windows Magnification API
- Optional crosshair overlay
- Optional PNG reticle support
- Circular scope mode
- Negative contrast mode
- Adjustable opacity, border thickness, zoom factor, and window size
- X/Y offset controls for the center marker
- System tray icon with settings and exit options
- Saves settings in the current user registry

## Controls

- **Right Mouse Button**: hold to zoom
- **Left Shift + Right Mouse Button**: toggle the tool on or off
- **Plus / Minus**: adjust zoom while active
- **Tray icon**: open settings or exit the app

## Settings

The settings window lets you change:

- Zoom factor
- Box size
- Opacity
- Border thickness
- Circular scope mode
- Negative mode
- Crosshair visibility
- Crosshair size
- X offset
- Y offset
- PNG reticle path

## Intended Use

This tool is designed for general desktop use and game scenarios where overlays are permitted.

## How it works

The app creates:
- a magnifier host window
- a crosshair window
- a settings window
- a system tray icon

A background thread polls input and updates the overlay position and magnified region while zoom is active.

## Build Requirements

- Windows
- Visual Studio or another MSVC-compatible compiler
- Windows SDK
- GDI+
- Windows Magnification API support

### Linked libraries

The project uses:

- `comctl32.lib`
- `advapi32.lib`
- `gdiplus.lib`
- `ole32.lib`
- `comdlg32.lib`

## Build Instructions

1. Open the x64 Native Tools Command Prompt for Visual Studio.
2. Make sure the Windows SDK is installed.
3. Build the project with:

```bat
cl.exe /W4 /EHsc /O2 zoom_tool.cpp /link Magnification.lib User32.lib Gdi32.lib Shell32.lib Comctl32.lib Advapi32.lib Gdiplus.lib Ole32.lib Comdlg32.lib /SUBSYSTEM:WINDOWS
```
4. Run the executable.
5. Open the tray menu to access settings.

## Registry Storage

Settings are stored under:

`HKCU\Software\PrecisionZoomTool`

## Notes

- IMPORTANT: This tool uses layered topmost windows and screen magnification behavior, so some games or strict anti-cheat systems may restrict it.
- It does not inject into other processes or modify game memory.
- PNG reticle loading is optional. If no PNG is loaded and crosshair is enabled, the app falls back to a built-in crosshair.

## Project Status

This project is functional and ready for further polish.

## Development Note

This project was initially generated with the assistance of AI and then reviewed and refined.

All code has been reviewed and tested manually.

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
