# CLAUDE.md

Windows-only Rust app (Win32 via `windows-sys`, settings UI via `eframe`/egui). Features, layout and build steps are in README.md; this file covers what isn't obvious from the code.

## Commands
- `.\build.ps1` runs the full check (fmt --check, clippy `-D warnings`, tests, release build into `dist\`). CI runs the same steps.
- `cargo fmt` uses default settings; clippy must pass with `-D warnings`.
- Releases: bump `version` in `Cargo.toml`, merge, push a `vX.Y.Z` tag. CI attaches `precision-zoom.exe` to the GitHub release.

## Invariants
- All window/magnifier work happens on the main (UI) thread. `ticker.rs` only posts `WM_APP_TICK`; never touch the zoom windows from another thread.
- App state lives in a thread-local `RefCell` accessed via `with_app` (`try_borrow_mut`, skipped if busy). Never hold the borrow across a modal call (`TrackPopupMenu`, `MessageBoxW`): ticks keep being dispatched inside modal loops.
- The settings window (`ui.rs`) runs eframe on its own thread (`with_any_thread`). It talks to the controller only via posted messages (`WM_APP_APPLY` carries a `Box<Config>`, `WM_APP_SET_ENABLED`) and reads app state from `ui::SHARED`. The app calls `ui::publish` after any change the UI didn't make (hotkey zoom, tray toggle); `external: false` publishes must not overwrite in-progress UI edits.
- winit can't create a second event loop per process: closing the settings window must only hide it (`CancelClose` + `Visible(false)`). `ui::show` wakes the hidden window via `request_repaint`, which runs `App::logic` (eframe 0.37 skips `App::ui` while hidden).
- The controller saves to the registry on a debounce (`schedule_save`, ~600 ms), not on every change.
- `App::show` must call `MagSetWindowFilterList` *after* the host and overlay windows are shown. The filter is ignored for windows hidden when it is set, and the reticle then gets magnified ("ghosting"). No unit test catches this; verify live after touching `show()`.
- Settings are stored one registry value per setting under `HKCU\Software\PrecisionZoomTool`. Add new fields to `load_from`, `save_to` and `sanitize`, with a default, and never change the meaning of an existing value name. The legacy `Settings` binary blob (C++ version) is migrated once by `from_legacy_blob`.
- Segoe UI (loaded from `%WINDIR%\Fonts`) lacks some symbols such as `✕` and `✓`, which render as boxes. Check glyphs in a screenshot when adding symbols to UI text.

## Live testing
- The registry key holds the user's real settings: back it up (`reg export`) before tests and restore it after.
- Screenshots: capture only the app's own windows with `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)`. Never use screen-region capture (`CopyFromScreen`/`BitBlt` of the desktop): the app window may be behind other windows, and it captures whatever the user has on screen.
- Drive the settings UI without moving the real cursor by posting `WM_MOUSEMOVE`/`WM_LBUTTONDOWN`/`WM_LBUTTONUP` with client coordinates to the egui window (find it by process id; its class isn't `PrecisionZoom*`).
- Don't inject right-clicks to trigger zoom. Bind the zoom key to Right Ctrl (`0xA3`, via the UI or `TriggerKey` in the registry before launch), inject that key with `keybd_event`, then restore `TriggerKey` = 2 and `ZoomX100`.
- Ghosting check: while zoomed, count reticle-colored pixels on the center row of the magnifier (the overlay alone gives `reticle_size + 1`; about `zoom` times that means the overlay is being magnified). This needs screen pixels, so count them in memory and don't save or view the image.
- In PowerShell P/Invoke, pass `[NullString]::Value`, not `$null`, for null string arguments (e.g. `FindWindow` titles); `$null` becomes `""`.
- Stop the running instance by posting `WM_COMMAND` with id 3 (tray Exit) to the `PrecisionZoomController` window; a second launch only opens the first instance's settings.
