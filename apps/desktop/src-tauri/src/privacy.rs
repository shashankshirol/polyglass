//! The privacy policy, linked from Settings (OSS-03). `PRIVACY.md` is built into the app, so
//! the text shown is the one this version shipped with and reading it needs no network: the
//! Settings window's "Privacy policy…" saves it as a text file in the app's data folder and
//! opens that file in the default text editor.

use std::path::{Path, PathBuf};

/// The repository's `PRIVACY.md` at build time.
pub const TEXT: &str = include_str!("../../../../PRIVACY.md");

/// File name under the app's local data directory. `.txt`, because every Windows installation
/// has a program for it; `.md` usually has none and would open the "choose an app" dialog.
pub const FILE: &str = "PRIVACY.txt";

/// Save the policy as [`FILE`] in `dir`, replacing a copy from an earlier version.
pub fn write(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE);
    std::fs::write(&path, TEXT)?;
    Ok(path)
}

/// Save the policy in `dir` and open it. The target is fixed: nothing from a webview reaches
/// the process arguments (SEC-01).
pub fn open(dir: &Path) -> std::io::Result<()> {
    let path = write(dir)?;
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oss03_the_built_in_policy_is_the_repository_file() {
        assert!(TEXT.starts_with("# Privacy"), "{:?}", &TEXT[..40]);
        // The statements the Settings window's summary repeats.
        for want in ["Nothing is sent anywhere", "No analytics, telemetry"] {
            assert!(TEXT.contains(want), "{want:?}");
        }
    }

    #[test]
    fn oss03_the_policy_is_saved_as_a_text_file_in_the_given_folder() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("not-there-yet");
        let path = write(&data).unwrap();
        assert_eq!(path, data.join("PRIVACY.txt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEXT);
        // An older copy is replaced, and nothing else in the folder is touched.
        std::fs::write(&path, "old").unwrap();
        std::fs::write(data.join("settings.toml"), "mine").unwrap();
        write(&data).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEXT);
        assert_eq!(
            std::fs::read_to_string(data.join("settings.toml")).unwrap(),
            "mine"
        );
    }
}
