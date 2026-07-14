//! Per-device action journal.
//!
//! Every destructive ADB action (disable, uninstall, clear, ...) is
//! appended to a JSON-Lines file. This is the substrate for the
//! "Activity" tab and the universal undo design tenet.
//!
//! ## Why JSON-Lines, not SQLite
//!
//! - **HGFS-safe**: SQLite is pathologically slow on VMware Shared
//!   Folders due to fsync overhead. JSONL is append-only and tolerant.
//! - **Diffable**: a user can grep / cat / mail the file. Bug-report
//!   attachments are plain text.
//! - **No dependency**: serde_json is already in the tree.
//! - **Append-only**: every line is independent — partial writes lose
//!   at most the last line, never the whole journal.
//!
//! We can revisit if cross-device journals or rich queries demand
//! SQLite, but for v0.1 the simplicity wins.
//!
//! ## File layout
//!
//! `<app_data_dir>/journal/<serial>.jsonl`, one [`JournalEntry`] per
//! line. The directory is created on demand. Per-device files isolate
//! noise — a wedged file lock on one device doesn't poison others.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::adb::actions::{ActionRequest, AppliedAction};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonic per-file id. Unique within a journal file.
    pub id: u64,
    pub applied: AppliedAction,
    /// `Some(undo_entry_id)` if a subsequent entry reverses this one
    /// via [`record_undo`].
    pub undone_by: Option<u64>,
    /// `Some(undone_entry_id)` if this entry was created to undo an
    /// earlier one. Lets the UI render an "Undo of #N" hint.
    pub undoes: Option<u64>,
}

/// In-memory journal. Loaded on demand, persisted line-by-line.
pub struct Journal {
    path: PathBuf,
    entries: Vec<JournalEntry>,
    next_id: AtomicU64,
}

impl Journal {
    /// Open the journal for `serial` under `dir`. Creates the directory
    /// if missing; replays any existing JSONL into memory.
    pub fn open(dir: &Path, serial: &str) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.jsonl", safe_serial(serial)));
        let mut entries: Vec<JournalEntry> = Vec::new();
        let mut max_id = 0u64;
        if path.exists() {
            let f = File::open(&path)?;
            for line in BufReader::new(f).lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<JournalEntry>(&line) {
                    Ok(e) => {
                        max_id = max_id.max(e.id);
                        if let Some(existing) = entries.iter_mut().find(|entry| entry.id == e.id) {
                            *existing = e;
                        } else {
                            entries.push(e);
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "[journal] dropping corrupt line in {path:?}: {err} (line preserved at end)"
                        );
                        // Don't blow up — corrupt lines are surfaced as
                        // dropped but we keep parsing the rest.
                    }
                }
            }
        }
        let next_id = max_id + 1;
        Ok(Self {
            path,
            entries,
            next_id: AtomicU64::new(next_id),
        })
    }

    /// On-disk path. Used by the future Settings → Diagnostics screen
    /// ("where is my data?") and useful in bug reports.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    /// Append an applied action.
    pub fn record(&mut self, applied: AppliedAction) -> std::io::Result<&JournalEntry> {
        let entry = JournalEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            applied,
            undone_by: None,
            undoes: None,
        };
        self.append(entry)
    }

    /// Append an undo entry for an existing entry, then mark the
    /// original as `undone_by`. Returns the new entry. Caller must have
    /// already executed the inverse `apply()` against the transport.
    pub fn record_undo(
        &mut self,
        original_id: u64,
        applied: AppliedAction,
    ) -> std::io::Result<&JournalEntry> {
        if !self.entries.iter().any(|e| e.id == original_id) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("journal has no entry id {original_id}"),
            ));
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let undo_entry = JournalEntry {
            id,
            applied,
            undone_by: None,
            undoes: Some(original_id),
        };
        self.append(undo_entry)?;

        // Patch the original in-memory and rewrite the file. Append-only
        // semantics for the on-disk format are preserved by writing a
        // *new* line with the patched original (the loader returns the
        // last-seen state per id, which matches this rewrite).
        if let Some(orig) = self.entries.iter_mut().find(|e| e.id == original_id) {
            orig.undone_by = Some(id);
            let patched = orig.clone();
            self.persist_line(&patched)?;
        }

        // Safe: we just pushed the undo entry; the entries Vec was
        // appended to inside `append`.
        Ok(self.entries.last().expect("we just appended"))
    }

    fn append(&mut self, entry: JournalEntry) -> std::io::Result<&JournalEntry> {
        self.persist_line(&entry)?;
        self.entries.push(entry);
        Ok(self.entries.last().expect("just pushed"))
    }

    fn persist_line(&self, entry: &JournalEntry) -> std::io::Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(f, "{line}")?;
        Ok(())
    }
}

/// Build a filename-safe variant of a device serial. Wireless serials
/// look like `192.168.1.42:5555` which is invalid as a Windows filename
/// (`:` is illegal). Escape problematic chars instead of replacing them
/// so `a:b` and `a_b` never collide.
fn safe_serial(serial: &str) -> String {
    let mut out = String::with_capacity(serial.len());
    for c in serial.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push_str("_x");
            out.push_str(&format!("{:X}", c as u32));
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// Given an entry id and the journal it lives in, synthesise the
/// [`ActionRequest`] that would undo it. Returns `None` if the action
/// kind isn't losslessly reversible.
pub fn undo_request_for(journal: &Journal, entry_id: u64) -> Option<ActionRequest> {
    let entry = journal.entries.iter().find(|e| e.id == entry_id)?;
    if entry.undone_by.is_some() {
        return None; // already undone
    }
    let kind = entry.applied.plan.request.kind.inverse()?;
    Some(ActionRequest {
        serial: entry.applied.plan.request.serial.clone(),
        package: entry.applied.plan.request.package.clone(),
        kind,
        // Undo must target the exact same Android user the original
        // action mutated, or a work-profile disable would re-enable on
        // the owner instead.
        user_id: entry.applied.plan.request.user_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::actions::{plan, ActionKind, ActionRequest, PlannedAction};
    use std::sync::Mutex;

    static TMP_COUNTER: Mutex<u32> = Mutex::new(0);

    fn fresh_tmp_dir(name: &str) -> PathBuf {
        let mut c = TMP_COUNTER.lock().unwrap();
        *c += 1;
        let dir = std::env::temp_dir().join(format!("droidsmith-journal-{name}-{}", *c));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn fake_applied(serial: &str, pkg: &str, kind: ActionKind) -> AppliedAction {
        AppliedAction {
            plan: plan(ActionRequest {
                serial: serial.into(),
                package: pkg.into(),
                kind,
                user_id: 0,
            }),
            stdout: "Package x new state: disabled\n".into(),
            applied_at: "2026-05-25T12:00:00Z".into(),
        }
    }

    #[test]
    fn record_and_reload_roundtrip() {
        let dir = fresh_tmp_dir("rt");
        let mut j = Journal::open(&dir, "abc").unwrap();
        j.record(fake_applied("abc", "com.foo", ActionKind::Disable))
            .unwrap();
        j.record(fake_applied("abc", "com.bar", ActionKind::Disable))
            .unwrap();
        drop(j);

        let j2 = Journal::open(&dir, "abc").unwrap();
        assert_eq!(j2.entries().len(), 2);
        assert_eq!(j2.entries()[0].id, 1);
        assert_eq!(j2.entries()[1].id, 2);
    }

    #[test]
    fn undo_request_for_inverts_kind() {
        let dir = fresh_tmp_dir("inv");
        let mut j = Journal::open(&dir, "abc").unwrap();
        j.record(fake_applied("abc", "com.foo", ActionKind::Disable))
            .unwrap();
        let req = undo_request_for(&j, 1).unwrap();
        assert_eq!(req.kind, ActionKind::Enable);
        assert_eq!(req.package, "com.foo");
    }

    #[test]
    fn undo_request_returns_none_for_irreversible() {
        let dir = fresh_tmp_dir("irr");
        let mut j = Journal::open(&dir, "abc").unwrap();
        j.record(fake_applied("abc", "com.foo", ActionKind::UninstallForUser))
            .unwrap();
        assert!(undo_request_for(&j, 1).is_none());
    }

    #[test]
    fn record_undo_links_both_sides() {
        let dir = fresh_tmp_dir("link");
        let mut j = Journal::open(&dir, "abc").unwrap();
        j.record(fake_applied("abc", "com.foo", ActionKind::Disable))
            .unwrap();
        let undo = fake_applied("abc", "com.foo", ActionKind::Enable);
        j.record_undo(1, undo).unwrap();

        let orig = j.entries().iter().find(|e| e.id == 1).unwrap();
        let new = j.entries().iter().find(|e| e.id == 2).unwrap();
        assert_eq!(orig.undone_by, Some(2));
        assert_eq!(new.undoes, Some(1));
    }

    #[test]
    fn reload_after_undo_keeps_last_state_per_id() {
        let dir = fresh_tmp_dir("link-reload");
        let mut j = Journal::open(&dir, "abc").unwrap();
        j.record(fake_applied("abc", "com.foo", ActionKind::Disable))
            .unwrap();
        j.record_undo(1, fake_applied("abc", "com.foo", ActionKind::Enable))
            .unwrap();
        drop(j);

        let mut reloaded = Journal::open(&dir, "abc").unwrap();
        assert_eq!(reloaded.entries().len(), 2);
        assert_eq!(reloaded.entries()[0].id, 1);
        assert_eq!(reloaded.entries()[0].undone_by, Some(2));
        assert_eq!(reloaded.entries()[1].id, 2);
        reloaded
            .record(fake_applied("abc", "com.bar", ActionKind::Disable))
            .unwrap();
        assert_eq!(reloaded.entries().last().unwrap().id, 3);
    }

    #[test]
    fn safe_serial_handles_wireless_colons() {
        assert_eq!(safe_serial("192.168.1.42:5555"), "192.168.1.42_x3A_5555");
        assert_eq!(safe_serial("abc-123_def.0"), "abc-123_def.0");
        assert_ne!(safe_serial("a:b"), safe_serial("a_b"));
        assert_eq!(safe_serial(""), "unknown");
    }

    #[test]
    fn corrupt_line_does_not_block_load() {
        let dir = fresh_tmp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.jsonl");
        let good: PlannedAction = plan(ActionRequest {
            serial: "abc".into(),
            package: "com.foo".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        let entry = JournalEntry {
            id: 1,
            applied: AppliedAction {
                plan: good,
                stdout: "ok".into(),
                applied_at: "2026-05-25T12:00:00Z".into(),
            },
            undone_by: None,
            undoes: None,
        };
        let mut f = File::create(&path).unwrap();
        writeln!(f, "{{this is not valid json").unwrap();
        writeln!(f, "{}", serde_json::to_string(&entry).unwrap()).unwrap();
        drop(f);

        let j = Journal::open(&dir, "abc").unwrap();
        // The corrupt line was dropped, the good one survived.
        assert_eq!(j.entries().len(), 1);
        assert_eq!(j.entries()[0].id, 1);
    }
}
