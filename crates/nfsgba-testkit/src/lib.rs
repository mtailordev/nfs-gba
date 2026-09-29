//! Shared test helpers (`docs/engine/testkit.md`). Every test reaches the user's ROM and the recorded fixtures
//! through [`rom`] and [`fixture`], so missing data follows one rule: with `NFSGBA_REQUIRE_DATA=1` it fails
//! the test (the merge gate, `tools/gate.py`, sets it); otherwise the test prints one `SKIPPED (no data)` line
//! and passes. With `NFSGBA_FIXTURE_LOG=<file>` every fixture resolved is appended to that file, which
//! `tools/fixtures.py` uses to build the fixture manifest (`docs/engine/fixtures.csv`).

use std::{env, fs, io::Write, path::PathBuf};

/// `$NFSGBA_DATA` (`nfsgba_formats::data_dir`).
pub fn data_dir() -> PathBuf {
    nfsgba_formats::data_dir()
}

/// Whether missing data fails the test (`NFSGBA_REQUIRE_DATA=1`).
pub fn data_required() -> bool {
    env::var("NFSGBA_REQUIRE_DATA").is_ok_and(|v| v == "1")
}

/// Missing test data: panics when data is required, else prints one skip line. Always `None`.
pub fn missing<T>(what: &str) -> Option<T> {
    assert!(!data_required(), "required test data missing: {what}");
    eprintln!("SKIPPED (no data): {what}");
    None
}

/// The canonical ROM (`data/vault`, built by `tools/vault.py`).
pub fn rom() -> Option<Vec<u8>> {
    match nfsgba_formats::canonical_rom() {
        Ok(rom) => Some(rom),
        Err(e) => missing(&format!("the canonical ROM ({e}; run tools/vault.py)")),
    }
}

/// `$NFSGBA_DATA/work/<first 8 hex digits of the canonical ROM's SHA-1>/`, where every fixture lives.
pub fn work_dir() -> Option<PathBuf> {
    let data = data_dir();
    let target = fs::read(data.join("vault/manifest.json"))
        .ok()
        .and_then(|m| serde_json::from_slice::<serde_json::Value>(&m).ok())
        .and_then(|m| m["canonical_target"].as_str().map(|s| s[..8].to_owned()));
    match target {
        Some(sha8) => Some(data.join("work").join(sha8)),
        None => missing("the ROM vault manifest (data/vault/manifest.json)"),
    }
}

/// A recorded fixture, file or folder, by its path under [`work_dir`] (e.g. `mgba/race.wram.bin`,
/// `vehicle-physics`). This is the only place a fixture path is resolved.
pub fn fixture(rel: &str) -> Option<PathBuf> {
    let path = work_dir()?.join(rel);
    if !path.exists() {
        return missing(&format!("fixture {rel}"));
    }
    if let Ok(log) = env::var("NFSGBA_FIXTURE_LOG") {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .expect("NFSGBA_FIXTURE_LOG");
        // One write per line: test threads append to the same file, and an append is atomic per write.
        f.write_all(format!("{rel}\n").as_bytes()).expect("NFSGBA_FIXTURE_LOG");
    }
    Some(path)
}

/// An mGBA dump by its prefix (`mgba/race` for `mgba/race.wram.bin`, `.iwram.bin`, …): the prefix path, when
/// its EWRAM file exists. Every domain file present is resolved through [`fixture`].
pub fn dump(prefix: &str) -> Option<PathBuf> {
    fixture(&format!("{prefix}.wram.bin"))?;
    for domain in ["iwram", "io", "palette", "vram", "oam", "bios", "eeprom"] {
        let rel = format!("{prefix}.{domain}.bin");
        if work_dir()?.join(&rel).exists() {
            fixture(&rel);
        }
    }
    Some(work_dir()?.join(prefix))
}

/// Counts what a replay checked and fails the test if it ends with any other count, including an early
/// `return` or `break` that skips the assertions after a loop: `let mut n = Expect::new("live free run", 699);`
/// then `n.tick()` per checked frame.
pub struct Expect {
    what: String,
    want: usize,
    got: usize,
}

impl Expect {
    pub fn new(what: impl Into<String>, want: usize) -> Expect {
        Expect {
            what: what.into(),
            want,
            got: 0,
        }
    }

    pub fn tick(&mut self) {
        self.got += 1;
    }
}

impl Drop for Expect {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(self.got, self.want, "{}: items checked", self.what);
        }
    }
}

/// The bytes of a fixture file ([`fixture`]).
pub fn read(rel: &str) -> Option<Vec<u8>> {
    let path = fixture(rel)?;
    Some(fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// The text of a fixture file ([`fixture`]).
pub fn read_to_string(rel: &str) -> Option<String> {
    let path = fixture(rel)?;
    Some(fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}
