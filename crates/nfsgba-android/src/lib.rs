//! The game as an Android app: the native library (`libnfsgba.so`) that the app's GameActivity loads
//! (`android/app/src/main/java/.../GameActivity.kt`). The launcher activity has copied the player's ROM into the app's
//! files before this starts (`nfsgba_viewer` `platform`). Everything else is the desktop's and the web's game.
#![cfg(target_os = "android")]

#[bevy::prelude::bevy_main]
fn main() {
    nfsgba_viewer::run();
}
