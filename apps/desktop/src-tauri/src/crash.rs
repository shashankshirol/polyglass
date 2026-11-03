//! Local crash reports (M1-APP-11, PRIV-06). A panic hook writes a plain-text report into
//! `<app local data>/crash-reports/` and nothing is sent anywhere: users attach a report to an
//! issue themselves (the tray's "Crash reports…" opens the folder). The newest [`KEEP`] are kept.
//!
//! A report holds the version, OS, thread, the panic's source location and message, and a
//! backtrace (release builds strip symbols, so it is mostly addresses there). It never holds
//! screen text unless a panic message itself carries some, which the app's own code doesn't do.
//! Crashes outside Rust (a fault inside CTranslate2's C++, say) end the process without a report.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::devflags::utc_stamp;

/// Folder under the app's local data directory.
pub const DIR: &str = "crash-reports";
/// Reports kept; older ones are deleted when a new one is written.
pub const KEEP: usize = 10;

/// What a report says about one panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub thread: String,
    pub location: String,
    pub message: String,
    pub backtrace: String,
}

impl Report {
    fn from_panic(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        Self {
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned(),
            location: info
                .location()
                .map_or_else(|| "unknown".into(), ToString::to_string),
            message,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        }
    }

    fn text(&self, at: SystemTime) -> String {
        format!(
            "Polyglass crash report\n\
             Saved on this computer only; nothing was sent. Attach this file to an issue if you\n\
             like (https://github.com/shashankshirol/polyglass/issues).\n\n\
             version:  {}\n\
             os:       {} {}\n\
             time:     {}\n\
             thread:   {}\n\
             location: {}\n\
             message:  {}\n\n\
             backtrace:\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            utc_stamp(at),
            self.thread,
            self.location,
            self.message,
            self.backtrace
        )
    }
}

/// Install the panic hook: every panic, on any thread, saves a report in `dir`, then the default
/// hook runs as before (it prints the panic to stderr).
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        match write(&dir, &Report::from_panic(info), SystemTime::now()) {
            Ok(path) => tracing::error!(report = %path.display(), "panic; crash report saved"),
            Err(error) => tracing::error!(%error, "panic; the crash report couldn't be saved"),
        }
        previous(info);
    }));
}

/// Save `report` as `crash-<UTC time>-<pid>.txt` in `dir` and delete all but the newest
/// [`KEEP`] reports.
pub fn write(dir: &Path, report: &Report, at: SystemTime) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stem = format!("crash-{}-{}", utc_stamp(at), std::process::id());
    // Two panics within a second get -2, -3, … rather than overwriting each other.
    let path = (1..)
        .map(|n| match n {
            1 => dir.join(format!("{stem}.txt")),
            n => dir.join(format!("{stem}-{n}.txt")),
        })
        .find(|p| !p.exists())
        .expect("an unused name");
    std::fs::write(&path, report.text(at))?;
    let all = reports(dir);
    for old in all.iter().take(all.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

/// The reports in `dir`, oldest first (the names sort by time).
pub fn reports(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    found.sort();
    found
}

/// Show `dir` in the system file manager.
pub fn open_folder(dir: &Path) -> std::io::Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(dir)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn report(message: &str) -> Report {
        Report {
            thread: "main".into(),
            location: "src/main.rs:1:1".into(),
            message: message.into(),
            backtrace: "0: main".into(),
        }
    }

    #[test]
    fn priv06_reports_are_local_files_and_only_the_newest_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_534_461);
        let first = write(dir.path(), &report("boom"), t0).unwrap();
        let text = std::fs::read_to_string(&first).unwrap();
        for want in [
            "nothing was sent",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            "2026-11-01T12-01-01Z",
            "location: src/main.rs:1:1",
            "message:  boom",
            "0: main",
        ] {
            assert!(text.contains(want), "{want:?} in {text}");
        }
        for i in 1..=KEEP as u64 + 2 {
            write(dir.path(), &report("again"), t0 + Duration::from_secs(i)).unwrap();
        }
        let kept = reports(dir.path());
        assert_eq!(kept.len(), KEEP);
        assert!(!first.exists(), "the oldest went first");
        // Other files in the folder are never touched.
        std::fs::write(dir.path().join("notes.txt"), "mine").unwrap();
        write(dir.path(), &report("more"), t0 + Duration::from_secs(99)).unwrap();
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn priv06_a_panic_on_any_thread_saves_a_report() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path().to_owned());
        let joined = std::thread::Builder::new()
            .name("crash-test".into())
            .spawn(|| panic!("crash-test panic {}", 42))
            .unwrap()
            .join();
        // Put the default hook back for the other tests.
        drop(std::panic::take_hook());
        assert!(joined.is_err());
        let texts: Vec<String> = reports(dir.path())
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("thread:   crash-test")
                && t.contains("message:  crash-test panic 42")
                && t.contains("crash.rs")),
            "{texts:?}"
        );
    }
}
