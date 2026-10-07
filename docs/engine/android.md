# Android port: plan

The game, rules and renderer are already platform-free (`nfsgba-game`, `nfsgba-formats`, `nfsgba-audio`, `nfsgba-sim`); the web build proved the viewer runs without a file system or `NFSGBA_*` variables. Android is a third platform for the same viewer: a native `.so` loaded by an Android activity, the player's own ROM picked on the phone, saves in the app's storage.

## What carries over unchanged

- The whole game (`Session`), the renderer (`indexed.wgsl`, the composite pass), audio (`GbaSound` through Bevy audio: AAudio on Android), the touch-pad layout and key mapping (as Bevy UI instead of HTML).
- The rule that the app ships only the engine: no ROM, no assets; the SHA-1 check (`e5298b24…`) as on the web.

## What is new

| Area | Desktop / web now | Android |
|---|---|---|
| Entry | `main()` / wasm-bindgen start | `#[bevy_main]` in a `cdylib`; Bevy feature `android-game-activity` (not a default feature since 0.19) |
| Packaging | cargo / `tools/web_build.py` | `cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build --profile mobile`, then Gradle (`android/`, a GameActivity app, min SDK 26) |
| ROM | file path / `window.nfsgba.rom` | first start: Storage Access Framework picker (`ACTION_OPEN_DOCUMENT`, .gba or .zip), SHA-1 checked, copied into app-internal storage; later starts read it from there |
| Save | `viewer.sav` / `localStorage` | `viewer.sav` in the app's internal data path, written on every game save and on suspend; export/import through the share sheet / SAF |
| Input | keyboard, gamepad, HTML touch pad | Bevy UI touch pad (D-pad, A, B, L, R, START, SELECT; multi-touch), gamepads (to verify: gilrs on Android, else GameActivity key events) |
| Lifecycle | none | `AppLifecycle`: pause the game and the sound on suspend, save, rebuild the surface on resume (Bevy handles the surface) |
| Screen | window / 3:2 canvas | landscape, fullscreen (status bar hidden), the 3:2 view letterboxed; menus at the largest whole scale; `WinitSettings` for mobile |
| GPU | Vulkan/DX12/Metal, WebGL2 | Vulkan (GLES 3 fallback): integer textures, `textureLoad`, `frag_depth` all exist there; MSAA off (the Bevy example turns it off for some devices) |

## Steps (each ends in something that runs)

1. **Spike (1 session):** a `platform` module that generalises today's `web` module (ROM bytes, save in, save out, one `cfg` per platform), the viewer as `lib` + `cdylib`, `android/` Gradle project from Bevy's `examples/mobile`, a debug APK that boots to the title screen with a ROM pushed by `adb` into the app's storage. Measure the frame time on a mid-range phone. Check gamepads here.
2. **ROM and save:** the SAF picker activity (small Kotlin, calls into Rust with the copied file's path), SHA-1 check with the web build's message, save to internal storage, export/import.
3. **Touch controls:** the web touch pad as Bevy UI, multi-touch, a setting to hide it when a gamepad is connected; haptics optional.
4. **Lifecycle and polish:** suspend/resume (pause, save, sound), orientation lock, immersive mode, app icon and name (no EA marks: an unofficial loader), battery: cap at 60 fps.
5. **Checks:** `cargo clippy --target aarch64-linux-android` in `tools/gate.py`; a CI job that builds the APK (artifact only: the APK holds no game data, but it is not published to a store); the existing headless tests already cover the game itself.

## Risks and open questions

- Gamepad support through gilrs on Android is unverified (step 1).
- Performance on low-end phones: the game step is cheap; the high-resolution renderer redraws the city per frame; fall back to the 240×160 frame (O) on weak GPUs.
- Store distribution: the app is an emulator-like loader for a commercial game's ROM; Play Store policy and EA's rights make a public store listing doubtful. Plan for side-loaded APKs (GitHub release) unless the user decides otherwise.
- iOS later reuses steps 1–4 (Bevy's `examples/mobile` has the iOS side too), but needs a Mac and signing.
