//! Another program of the same build, started in place of this one: a game hands over to the game
//! beside it (the flight simulator to the driving game, and back). The other program is the file
//! of that name in the folder of the running executable, where `cargo build` puts the programs of
//! a workspace and where a macOS bundle keeps them (`Contents/MacOS`).

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// The program `name` in `folder`, if it is there (`name.exe` on Windows).
pub fn in_folder(folder: &Path, name: &str) -> Option<PathBuf> {
    let path = folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

/// The program `name` beside the running executable, if it is there.
pub fn beside(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Through a link to the executable, the folder the executable is really in.
    let exe = exe.canonicalize().unwrap_or(exe);
    in_folder(exe.parent()?, name)
}

/// Starts the program at `path` with `arguments`. It is on its own from then on: it keeps this
/// one's folder, environment and terminal, and goes on when this one ends.
pub fn start(path: &Path, arguments: &[&str]) -> Result<Child, String> {
    Command::new(path).args(arguments).spawn().map_err(|error| format!("{}: {error}", path.display()))
}

/// Starts the program `name` beside this one with `arguments` and ends this one: only an error
/// comes back (the program is not there, or could not be started).
pub fn hand_over(name: &str, arguments: &[&str]) -> String {
    let Some(path) = beside(name) else {
        return format!("{name} was not found beside this program");
    };
    match start(&path, arguments) {
        Ok(_) => std::process::exit(0),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the tests' own, empty.
    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("engine-program-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_program_is_found_only_in_its_folder_and_only_if_it_is_a_file() {
        let folder = folder("found");
        std::fs::create_dir_all(folder.join("other")).unwrap();
        let file = folder.join(format!("game{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&file, b"").unwrap();
        assert_eq!(in_folder(&folder, "game"), Some(file));
        // A folder of that name is not a program, nor is one that is not there.
        assert_eq!(in_folder(&folder, "other"), None);
        assert_eq!(in_folder(&folder, "missing"), None);
        std::fs::remove_dir_all(&folder).unwrap();
        // Nothing of that name beside the tests' own program: the error says so, and the tests go on.
        assert_eq!(beside("no-such-program"), None);
        assert!(hand_over("no-such-program", &[]).contains("not found"));
    }

    /// The program found in a folder is started with the arguments given, in this one's folder.
    #[cfg(unix)]
    #[test]
    fn the_program_is_started_with_its_arguments() {
        use std::os::unix::fs::PermissionsExt;
        let folder = folder("started");
        let said = folder.join("said");
        let script = folder.join("game");
        std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" > '{}'\n", said.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = in_folder(&folder, "game").unwrap();
        assert!(start(&path, &["--menu", "vehicles"]).unwrap().wait().unwrap().success());
        assert_eq!(std::fs::read_to_string(&said).unwrap().trim(), "--menu vehicles");
        // What is not a program does not start, and the error names it.
        std::fs::write(folder.join("text"), b"not a program").unwrap();
        assert!(start(&folder.join("text"), &[]).unwrap_err().contains("text"));
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
