//! The types of aircraft the game has, read at start: every `assets/aircraft/<key>/aircraft.ron` found
//! on disk, in place of the built-in type of the same key or after them (`sim_core::aircraft::file`);
//! and the multirotors, every `assets/aircraft/<key>/multirotor.ron` (`sim_core::multirotor::file`).
//! Without the folder, the built-in ones: the files as they were when the game was compiled.
//!
//! The folder is the first of these that exists:
//! 1. `$FLIGHT_ASSETS/aircraft`;
//! 2. `assets/aircraft` next to the executable or in any folder above it;
//! 3. `assets/aircraft` in the current folder or any folder above it.

use sim_core::aircraft::file;
use sim_core::multirotor::file as multirotor_file;
use std::path::{Path, PathBuf};

/// Reads the types, checks that the game can draw them all and makes them the fleet; the message
/// for the player if a file is wrong.
pub fn load() -> Result<(), String> {
    let (files, multirotors) = match folder() {
        Some(dir) => (read(&dir, "aircraft.ron")?, read(&dir, "multirotor.ron")?),
        None => (Vec::new(), Vec::new()),
    };
    let fleet = file::fleet_with(&files).map_err(|e| e.to_string())?;
    crate::kinds::check(&fleet)?;
    let drones = multirotor_file::fleet_with(&multirotors).map_err(|e| e.to_string())?;
    crate::kinds::check_multirotors(&drones)?;
    // One word names one thing on the command line and in the settings (asked of the types read:
    // the fleet is not made yet).
    let taken: Vec<&str> = fleet.iter().flat_map(|t| std::iter::once(t.kind.key()).chain(t.aliases.iter().copied())).collect();
    for d in &drones {
        if let Some(word) = std::iter::once(d.kind.key()).chain(d.aliases.iter().copied()).find(|w| taken.contains(w)) {
            return Err(format!("the multirotor {} is called \"{word}\", as an aircraft is", d.kind.key()));
        }
    }
    file::set_fleet(fleet)?;
    multirotor_file::set_fleet(drones)
}

/// The bytes of `file` of the look `name` (a folder of the aircraft's: `<name>/exterior.glb`):
/// from the folder the types are read from, or on the web, where there is none, from the files the
/// page downloaded (`assets/aircraft/...`).
pub fn file(name: &str, file: &str) -> Result<Vec<u8>, String> {
    let path = path(name, file);
    engine::files::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the look `name` has `file`.
pub fn has(name: &str, file: &str) -> bool {
    engine::files::exists(&path(name, file))
}

fn path(name: &str, file: &str) -> PathBuf {
    folder().unwrap_or_else(|| PathBuf::from("assets/aircraft")).join(name).join(file)
}

/// The bytes of a file of the game's `assets` folder that is not an aircraft's (`menu/places/LEMH.jpg`):
/// the folder beside the aircraft's.
pub fn asset(file: &str) -> Result<Vec<u8>, String> {
    let path = folder().as_deref().and_then(Path::parent).map_or_else(|| PathBuf::from("assets"), Path::to_path_buf).join(file);
    engine::files::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The text of `file` of the look `name`, as `file`.
pub fn text(name: &str, file: &str) -> Result<String, String> {
    String::from_utf8(self::file(name, file)?).map_err(|e| format!("{name}/{file}: {e}"))
}

fn folder() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("FLIGHT_ASSETS") {
        candidates.push(Path::new(&dir).join("aircraft"));
    }
    let exe = std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok()).and_then(|exe| exe.parent().map(Path::to_path_buf));
    for start in [exe, std::env::current_dir().ok()].into_iter().flatten() {
        candidates.extend(start.ancestors().map(|dir| dir.join("assets/aircraft")));
    }
    candidates.into_iter().find(|dir| dir.is_dir())
}

/// Each `<key>/<name>` of the folder (`aircraft.ron`, `multirotor.ron`), in the order of the keys:
/// (its path, its text).
fn read(dir: &Path, name: &str) -> Result<Vec<(String, String)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path().join(name)).filter(|path| path.is_file()).collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| match std::fs::read_to_string(&path) {
            Ok(text) => Ok((path.display().to_string(), text)),
            Err(e) => Err(format!("{}: {e}", path.display())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository's folder holds the built-in types as they are now.
    #[test]
    fn the_repository_s_files_are_the_built_in_ones() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/aircraft");
        let files = read(&dir, "aircraft.ron").unwrap();
        let keys: Vec<&str> = files.iter().map(|(path, _)| path.rsplit('/').nth(1).unwrap()).collect();
        assert_eq!(keys, ["c172", "extra300"]);
        assert_eq!(file::fleet_with(&files).unwrap(), file::fleet());
        let drones = read(&dir, "multirotor.ron").unwrap();
        assert_eq!(drones.len(), 3);
        assert_eq!(multirotor_file::fleet_with(&drones).unwrap(), multirotor_file::fleet());
    }
}
