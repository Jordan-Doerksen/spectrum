//! Is the process that holds the lock still running?
//!
//! `tasklist` ships with Windows, so this costs no crate (D-0003). It is one call, at
//! startup only.

use super::record::Record;

/// The packaged control panel. Checked for a warning only — see the `lock` module note.
pub const PANEL_EXE: &str = "spectrum-pro.exe";
/// `tasklist` prints the image name in a 25-character column and cuts a longer name
/// there, with no ellipsis. Measured on this host, 2026-09-18.
const IMAGE_COLUMN: usize = 25;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead,
    /// The check could not answer — the heartbeat decides instead.
    Unknown,
}

impl Liveness {
    pub fn as_str(self) -> &'static str {
        match self {
            Liveness::Alive => "alive",
            Liveness::Dead => "dead",
            Liveness::Unknown => "unknown",
        }
    }
}

/// Ask Windows whether the recorded process still runs.
///
/// Two tests, in this order, and neither one reads localized text.
/// 1. **Is there a row at all?** The pid is printed in its own row, and the "no tasks
///    match" sentence holds no digits. No row means no such pid: the holder is dead.
/// 2. **Is it the same program?** A row proves only that SOME process owns the pid,
///    which a recycled pid also does. `tasklist` cuts the image column at
///    [`IMAGE_COLUMN`] characters with no ellipsis, so the recorded name is matched by
///    that prefix, never whole. A row whose name does not match is NOT called dead —
///    that answer frees the lock, and a wrong one starts a second poster. It is
///    `Unknown`, and the heartbeat decides.
pub fn probe(rec: &Record) -> Liveness {
    let Some(out) = tasklist(&format!("PID eq {}", rec.pid)) else {
        return Liveness::Unknown;
    };
    if !out.contains(&rec.pid.to_string()) {
        return Liveness::Dead;
    }
    let exe = rec.exe.to_ascii_lowercase();
    let printed: String = exe.chars().take(IMAGE_COLUMN).collect();
    if !printed.is_empty() && out.contains(&printed) {
        Liveness::Alive
    } else {
        Liveness::Unknown
    }
}

/// True when the packaged panel is running on this host. Warning only.
pub fn panel_running() -> bool {
    tasklist(&format!("IMAGENAME eq {PANEL_EXE}")).is_some_and(|out| out.contains(PANEL_EXE))
}

/// One `tasklist` call, lowercased. `None` means the command did not answer.
fn tasklist(filter: &str) -> Option<String> {
    let out = std::process::Command::new("tasklist")
        .args(["/NH", "/FI"])
        .arg(filter)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one check the injected seam cannot cover: `tasklist` itself. Both assertions
    /// hold where `tasklist` is missing too, where every answer is Unknown.
    #[test]
    fn the_liveness_probe_reads_this_host() {
        let me = Record::of_this_process(0);
        assert_ne!(probe(&me), Liveness::Dead, "the running test process must not read as dead");

        let ghost = Record { pid: u32::MAX - 6, ..me };
        assert_ne!(probe(&ghost), Liveness::Alive, "a pid that cannot exist must not read as alive");
    }

    /// The test binary's own name is longer than the `tasklist` column, so this asserts
    /// the truncation rule that an earlier version of `probe` got wrong: it read a live
    /// process as dead, which frees a held lock and starts a second poster.
    #[test]
    fn a_name_past_the_column_still_reads_as_alive() {
        let me = Record::of_this_process(0);
        assert!(me.exe.len() > IMAGE_COLUMN, "this test needs a long binary name: {}", me.exe);
        assert_eq!(probe(&me), Liveness::Alive);
    }
}
