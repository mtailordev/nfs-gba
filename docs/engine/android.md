# Android

The game as an Android app: the same `nfsgba_viewer::run()` as the desktop and the web, built as a native library that a GameActivity loads. What differs per platform is in `crates/nfsgba-viewer/src/platform.rs` (the ROM, the save, the window); input, pacing and lifecycle are shared code.

## Pieces

| Piece | What |
|---|---|
| `crates/nfsgba-android` | `cdylib` `libnfsgba.so`: `#[bevy_main]` calls `nfsgba_viewer::run()`. Bevy feature `android-game-activity` (not a default since 0.19) and `bevy_render/gles` (OpenGL ES 3 besides Vulkan). Empty on other targets, so the workspace gate stays as it was. |
| `android/` | Gradle project (AGP 8.4, Gradle 8.6, wrapper jar checked against Gradle's published SHA-256; `games-activity` 4.4.0, which must match Bevy's `android-activity` 0.6.1). Adapted from Bevy's `examples/mobile` (MIT/Apache-2.0). |
| `LauncherActivity` | Starts the game when `files/rom.gba` is there and has the canonical SHA-1; else asks for the ROM with the system file picker (`.gba`, or the first `.gba` in a `.zip`), checks the SHA-1 and copies it. |
| `MainActivity` | The GameActivity; immersive full screen; every configuration change handled in place (the native side runs one activity per process: a recreated activity would have no game). |
| `tools/android_build.py` | `cargo ndk` (arm64-v8a, x86_64) then Gradle; `--debug --install --push-rom --run` for a test on a device or the emulator. |
| `.github/workflows/builds.yml` | The release APK as a build artifact of every push (signed with the runner's debug key: side-loading only, and a new build may need the old one uninstalled). |

Shared with the other platforms: the touch pad (`touch.rs`, shown from the start on Android), controller buttons as Android key codes (`touch::android_button`; gilrs has no Android backend), pause in the background (`play::lifecycle`), the render scale (`composite::RenderScale`), the audio-clock sync (`play::sync_rate`).

## Checked (2026-10-07, emulator Pixel 3a API 34 x86_64, SwiftShader software Vulkan, headless)

- Power-on to a Quick Play race through the touch pad (adb taps), and the menus through controller key events (adb `KEYCODE_BUTTON_A`).
- Real speed with a slow GPU: the menus at 59.7 game frames/s (the original's 59.73) and the race at 14.9 (59.73 / 4) while the display ran at 25–27 fps; a race step 0.45 ms. The render scale stepped down to its floor, as it should on a software GPU.

## Open

- No real phone tested yet; no real controller (analog stick and hat-switch D-pads come as motion events, which are not read).
- A device with OpenGL ES only and no sRGB EGL surface (the emulator without Vulkan) fails at the surface: Bevy 0.19 asks for an sRGB view of a non-sRGB surface (`SURFACE_VIEW_FORMATS`). Real devices with Vulkan, and GLES devices with `EGL_KHR_gl_colorspace`, are not affected.
- No export/import of the save in the app yet (it is `files/viewer.sav`, the game's EEPROM image).
- iOS would reuse all of this (Bevy's `examples/mobile` has the Xcode side) but needs a Mac.
