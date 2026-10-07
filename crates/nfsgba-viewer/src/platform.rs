//! What differs between the platforms the game runs on: where the player's ROM comes from, where the save goes, and
//! the window. Everything else (the game, the renderer, the sound, the input) is the same code everywhere.
//!
//! - Desktop: the ROM given as the first argument (`nfsgba-viewer path/to/rom.gba`), else the vault's canonical ROM
//!   (`tools/vault.py`); the save at `NFSGBA_SAVE`, else next to the ROM argument, else
//!   `$NFSGBA_DATA/work/e5298b24/viewer.sav`.
//! - Web: the page (`web/index.html`) passes the ROM and the save in `window.nfsgba` (`rom`, `save`: byte arrays) and
//!   keeps every save the game writes (`window.nfsgba.saved(bytes)`) in browser storage.
//! - Android: the app's files directory, where the launcher activity copies the ROM the player picked
//!   (`rom.gba`, `android/`); the save next to it (`viewer.sav`).

use std::path::PathBuf;

use bevy::{prelude::*, window::WindowResolution};

/// The cartridge's game code (`0xAC`) and size: Need for Speed Carbon: Own the City.
pub fn is_carbon(rom: &[u8]) -> bool {
    rom.len() == 8 << 20 && rom.get(0xAC..0xB0) == Some(b"BN7E")
}

/// The player's ROM. Panics with what to do when there is none (the platforms' launchers check before starting).
pub fn rom() -> Vec<u8> {
    let rom = imp::rom();
    assert!(
        is_carbon(&rom),
        "not Need for Speed Carbon: Own the City (game code BN7E, 8 MiB)"
    );
    rom
}

/// Where the game's save (the cartridge's EEPROM image, 512 bytes) is kept.
pub struct Saves {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    path: Option<PathBuf>,
}

impl Saves {
    /// The platform's place for the save.
    pub fn here() -> Saves {
        Saves { path: imp::save_path() }
    }

    /// A save file at `path` (tests).
    #[cfg(test)]
    pub fn file(path: PathBuf) -> Saves {
        Saves { path: Some(path) }
    }

    /// Where the save is kept, for the log.
    pub fn place(&self) -> String {
        match &self.path {
            Some(path) => path.display().to_string(),
            None if cfg!(target_arch = "wasm32") => "the page's storage".into(),
            None => "nowhere".into(),
        }
    }

    /// The last save, if there is one.
    pub fn load(&self) -> Option<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        return imp::load_save();
        #[cfg(not(target_arch = "wasm32"))]
        return std::fs::read(self.path.as_ref()?).ok();
    }

    /// Keeps `eeprom` as the save.
    pub fn store(&self, eeprom: &[u8]) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        return imp::store_save(eeprom);
        #[cfg(not(target_arch = "wasm32"))]
        match &self.path {
            Some(path) => std::fs::write(path, eeprom).map_err(|e| format!("save {}: {e}", path.display())),
            None => Ok(()),
        }
    }
}

/// The game's window: four times the GBA screen on the desktop, the page's canvas on the web, the whole screen on a
/// phone.
pub fn window() -> Window {
    Window {
        title: "Need for Speed Carbon: Own the City (unofficial engine)".into(),
        resolution: imp::resolution(),
        mode: imp::mode(),
        // Web: the page's canvas, sized by the page.
        canvas: Some("#game".into()),
        fit_canvas_to_parent: true,
        ..default()
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod imp {
    use super::*;

    /// The first argument, if it is a file.
    fn argument() -> Option<PathBuf> {
        std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_file())
    }

    pub fn rom() -> Vec<u8> {
        match argument() {
            Some(path) => std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            None => nfsgba_formats::canonical_rom().unwrap_or_else(|e| {
                panic!("no ROM: pass your Need for Speed Carbon: Own the City .gba as the first argument ({e})")
            }),
        }
    }

    pub fn save_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("NFSGBA_SAVE") {
            return Some(p.into());
        }
        if let Some(rom) = argument() {
            return Some(rom.with_extension("sav"));
        }
        Some(nfsgba_formats::data_dir().join("work/e5298b24/viewer.sav"))
    }

    pub fn resolution() -> WindowResolution {
        // Four times the GBA screen: every GBA pixel is 4×4 window pixels.
        WindowResolution::new(960, 640)
    }

    pub fn mode() -> bevy::window::WindowMode {
        bevy::window::WindowMode::Windowed
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use js_sys::{Function, Reflect, Uint8Array, global};

    fn page(key: &str) -> Option<wasm_bindgen::JsValue> {
        let page = Reflect::get(&global(), &"nfsgba".into()).ok()?;
        Reflect::get(&page, &key.into())
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null())
    }

    pub fn rom() -> Vec<u8> {
        Uint8Array::new(&page("rom").expect("the page passes the ROM in window.nfsgba.rom")).to_vec()
    }

    pub fn save_path() -> Option<PathBuf> {
        None
    }

    pub fn load_save() -> Option<Vec<u8>> {
        page("save").map(|v| Uint8Array::new(&v).to_vec())
    }

    pub fn store_save(eeprom: &[u8]) -> Result<(), String> {
        match page("saved") {
            Some(f) => Function::from(f)
                .call1(&global(), &Uint8Array::from(eeprom))
                .map(|_| ())
                .map_err(|e| format!("the page did not take the save: {e:?}")),
            None => Ok(()),
        }
    }

    pub fn resolution() -> WindowResolution {
        // One canvas pixel per UI unit. On high-density screens the canvas comes out at its CSS size while the scale
        // factor is the screen's, so the menus (sized from the window's logical width) would overflow it.
        WindowResolution::new(960, 640).with_scale_factor_override(1.0)
    }

    pub fn mode() -> bevy::window::WindowMode {
        bevy::window::WindowMode::Windowed
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::*;

    /// The app's own files directory (internal storage, no permission needed).
    fn files() -> PathBuf {
        bevy::android::ANDROID_APP
            .get()
            .and_then(|app| app.internal_data_path())
            .expect("the app's files directory")
    }

    pub fn rom() -> Vec<u8> {
        let path = files().join("rom.gba");
        std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (the launcher copies the picked ROM there)", path.display()))
    }

    pub fn save_path() -> Option<PathBuf> {
        Some(files().join("viewer.sav"))
    }

    pub fn resolution() -> WindowResolution {
        WindowResolution::default()
    }

    pub fn mode() -> bevy::window::WindowMode {
        bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Primary)
    }
}
