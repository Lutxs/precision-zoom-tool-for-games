# CLAUDE.md

Windows-only Rust app (Win32 via `windows-sys`). Features, layout and build steps are in README.md; this file covers what isn't obvious from the code.

## Commands
- `.\build.ps1` runs the full check (fmt --check, clippy `-D warnings`, tests, release build into `dist\`). CI runs the same steps.
- `cargo fmt` uses default settings; clippy must pass with `-D warnings`.
- Releases: bump `version` in `Cargo.toml`, merge, push a `vX.Y.Z` tag. CI attaches `precision-zoom.exe` to the GitHub release.

## Invariants
- All window/magnifier work happens on the UI thread. `ticker.rs` only posts `WM_APP_TICK`; never touch windows from another thread.
- App and settings state live in thread-local `RefCell`s accessed via `with_app` / `with_state` (`try_borrow_mut`, skipped if busy). Never hold a borrow across a modal call (`TrackPopupMenu`, `GetOpenFileNameW`, `ChooseColorW`, `MessageBoxW`): ticks keep being dispatched inside modal loops.
- `App::show` must call `MagSetWindowFilterList` *after* the host and overlay windows are shown. The filter is ignored for windows hidden when it is set, and the reticle then gets magnified ("ghosting"). No unit test catches this; verify live after touching `show()`.
- Settings are stored one registry value per setting under `HKCU\Software\PrecisionZoomTool`. Add new fields to `load_from`, `save_to` and `sanitize`, with a default, and never change the meaning of an existing value name. The legacy `Settings` binary blob (C++ version) is migrated once by `from_legacy_blob`.

## Live testing
- The registry key holds the user's real settings: back it up (`reg export`) before tests and restore it after.
- Don't inject right-clicks to trigger zoom. Temporarily set `TriggerKey` to `0xA3` (Right Ctrl), inject that key with `keybd_event`, then restore `TriggerKey` = 2 and `ZoomX100`.
- Ghosting check: capture the screen around the cursor while zoomed and count reticle-colored pixels on the center row. The overlay alone gives `reticle_size + 1`; about `zoom` times that means the overlay is being magnified.
- In PowerShell P/Invoke, pass `[NullString]::Value`, not `$null`, for null string arguments (e.g. `FindWindow` titles); `$null` becomes `""`.
- Stop the running instance by posting `WM_COMMAND` with id 3 (tray Exit) to the `PrecisionZoomController` window; a second launch only opens the first instance's settings.
