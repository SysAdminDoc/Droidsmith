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
//! `<app_data_dir>/journal/<serial>.jsonl`, one immutable state transition per
//! line. Repeated ids form a write-ahead intent followed by one terminal
//! outcome; replay keeps the last durable transition. The directory is created
//! on demand. Per-device files isolate noise — a wedged file lock on one device
//! doesn't poison others.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::adb::actions::{ActionRequest, AppliedAction, PlannedAction};

/// Per-device lock so the open → read → record → write cycle is atomic.
/// Two concurrent `apply_action` calls on the same device would otherwise
/// each open an independent [`Journal`], both derive the same `next_id`
/// from the file, and append duplicate ids with stale undo state.
fn device_lock(serial: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(safe_serial(serial))
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Run `f` with exclusive access to `serial`'s journal. The closure
/// receives a freshly-opened journal; the whole open→mutate cycle holds
/// the per-device lock, so concurrent callers serialize instead of racing
/// on ids. Long work inside `f` (e.g. an undo's inverse ADB call) is
/// intentionally serialized per device — a device cannot meaningfully run
/// two package mutations at once anyway.
pub fn with_journal<T, E>(
    dir: &Path,
    serial: &str,
    f: impl FnOnce(&mut Journal) -> Result<T, E>,
) -> Result<T, E>
where
    E: From<std::io::Error>,
{
    let lock = device_lock(serial);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut journal = Journal::open(dir, serial)?;
    f(&mut journal)
}

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
    /// Durable lifecycle for this operation. Missing on legacy rows, which
    /// were only written after success and therefore deserialize as succeeded.
    #[serde(default)]
    pub outcome: JournalOutcome,
    /// Redacted terminal failure or recovery detail. Pending and successful
    /// entries keep this empty.
    #[serde(default)]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalOutcome {
    Pending,
    #[default]
    Succeeded,
    Failed,
    Interrupted,
}

#[derive(Debug)]
pub enum ExecuteError<E> {
    Journal(std::io::Error),
    Operation(E),
}

impl<E> From<std::io::Error> for ExecuteError<E> {
    fn from(error: std::io::Error) -> Self {
        Self::Journal(error)
    }
}

impl<E: std::fmt::Display> std::fmt::Display for ExecuteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Journal(error) => write!(f, "operation journal failed: {error}"),
            Self::Operation(error) => error.fmt(f),
        }
    }
}

/// Journal location used by the headless CLI. It mirrors Tauri's app-data
/// convention for the `com.droidsmith.app` identifier so GUI and CLI recovery
/// share one per-device log.
pub fn default_journal_dir() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local").join("share"))
        });
    base.map(|path| path.join("com.droidsmith.app").join("journal"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not resolve the Droidsmith app-data directory",
            )
        })
}

/// In-memory journal. Loaded on demand, persisted line-by-line.
pub struct Journal {
    path: PathBuf,
    entries: Vec<JournalEntry>,
    next_id: AtomicU64,
    #[cfg(test)]
    fail_persist: Option<PersistFailure>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistFailure {
    Open,
    Write,
    Flush,
    Sync,
    PartialWrite,
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
            repair_partial_tail(&path)?;
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
        let mut journal = Self {
            path,
            entries,
            next_id: AtomicU64::new(next_id),
            #[cfg(test)]
            fail_persist: None,
        };
        journal.rebuild_undo_links();
        journal.reconcile_interrupted()?;
        Ok(journal)
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

    /// Durably record intent before any device mutation is attempted.
    fn begin(
        &mut self,
        plan: PlannedAction,
        started_at: &str,
        undoes: Option<u64>,
    ) -> std::io::Result<u64> {
        if let Some(original_id) = undoes {
            if undo_request_for(self, original_id).is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("journal entry {original_id} is not safely undoable"),
                ));
            }
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = JournalEntry {
            id,
            applied: AppliedAction {
                before_state: if plan.before_state.is_empty() {
                    "unknown".to_string()
                } else {
                    plan.before_state.clone()
                },
                after_state: "not_applied".to_string(),
                plan,
                stdout: String::new(),
                display_stdout: String::new(),
                applied_at: started_at.to_string(),
            },
            undone_by: None,
            undoes,
            outcome: JournalOutcome::Pending,
            failure: None,
        };
        self.append(entry)?;
        self.rebuild_undo_links();
        Ok(id)
    }

    /// Complete a pending intent. Repeating the exact success transition is
    /// idempotent; conflicting terminal transitions are rejected.
    fn succeed(&mut self, id: u64, applied: AppliedAction) -> std::io::Result<&JournalEntry> {
        let current = self.entry(id)?.clone();
        if !same_plan(&current.applied.plan, &applied.plan) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("journal outcome does not match intent {id}"),
            ));
        }
        if current.outcome == JournalOutcome::Succeeded {
            return self.entry(id);
        }
        if current.outcome != JournalOutcome::Pending {
            return Err(invalid_transition(id, current.outcome));
        }
        let mut terminal = current;
        terminal.applied = applied;
        terminal.outcome = JournalOutcome::Succeeded;
        terminal.failure = None;
        self.replace(terminal)
    }

    fn fail(
        &mut self,
        id: u64,
        message: String,
        finished_at: &str,
    ) -> std::io::Result<&JournalEntry> {
        let current = self.entry(id)?.clone();
        if current.outcome == JournalOutcome::Failed {
            return self.entry(id);
        }
        if current.outcome != JournalOutcome::Pending {
            return Err(invalid_transition(id, current.outcome));
        }
        let mut terminal = current;
        terminal.outcome = JournalOutcome::Failed;
        terminal.failure = Some(message);
        terminal.applied.applied_at = finished_at.to_string();
        self.replace(terminal)
    }

    /// Execute one mutation between a durable intent and terminal outcome.
    /// A journal write/sync failure before `run` means `run` is never called;
    /// a terminal-write failure leaves the durable intent for recovery.
    pub fn execute<E: std::fmt::Display>(
        &mut self,
        plan: PlannedAction,
        undoes: Option<u64>,
        started_at: &str,
        run: impl FnOnce(PlannedAction) -> Result<AppliedAction, E>,
    ) -> Result<JournalEntry, ExecuteError<E>> {
        let id = self
            .begin(plan.clone(), started_at, undoes)
            .map_err(ExecuteError::Journal)?;
        let request = plan.request.clone();
        match run(plan) {
            Ok(applied) => self
                .succeed(id, applied)
                .cloned()
                .map_err(ExecuteError::Journal),
            Err(error) => {
                let failure =
                    crate::adb::actions::redact_journal_text(&request, &error.to_string());
                self.fail(id, failure, &crate::time::iso_utc_now())
                    .map_err(ExecuteError::Journal)?;
                Err(ExecuteError::Operation(error))
            }
        }
    }

    /// Compatibility helper for import/tests that already hold a completed
    /// action. Production mutation paths use [`Journal::execute`] so intent
    /// is durable before the device call.
    #[cfg(test)]
    fn record(&mut self, applied: AppliedAction) -> std::io::Result<&JournalEntry> {
        let id = self.begin(applied.plan.clone(), &applied.applied_at.clone(), None)?;
        self.succeed(id, applied)
    }

    /// Compatibility counterpart to [`Journal::record`].
    #[cfg(test)]
    fn record_undo(
        &mut self,
        original_id: u64,
        applied: AppliedAction,
    ) -> std::io::Result<&JournalEntry> {
        let id = self.begin(
            applied.plan.clone(),
            &applied.applied_at.clone(),
            Some(original_id),
        )?;
        self.succeed(id, applied)
    }

    fn append(&mut self, entry: JournalEntry) -> std::io::Result<&JournalEntry> {
        self.persist_line(&entry)?;
        self.entries.push(entry);
        Ok(self.entries.last().expect("just pushed"))
    }

    fn replace(&mut self, entry: JournalEntry) -> std::io::Result<&JournalEntry> {
        self.persist_line(&entry)?;
        let id = entry.id;
        let index = self
            .entries
            .iter()
            .position(|candidate| candidate.id == id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("journal has no entry id {id}"),
                )
            })?;
        self.entries[index] = entry;
        self.rebuild_undo_links();
        self.entries
            .iter()
            .find(|candidate| candidate.id == id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("journal has no entry id {id}"),
                )
            })
    }

    fn entry(&self, id: u64) -> std::io::Result<&JournalEntry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("journal has no entry id {id}"),
                )
            })
    }

    fn rebuild_undo_links(&mut self) {
        let known: std::collections::HashSet<u64> =
            self.entries.iter().map(|entry| entry.id).collect();
        for entry in &mut self.entries {
            if entry.undone_by.is_some_and(|id| known.contains(&id)) {
                entry.undone_by = None;
            }
        }
        let links: Vec<(u64, u64)> = self
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.outcome,
                    JournalOutcome::Pending
                        | JournalOutcome::Succeeded
                        | JournalOutcome::Interrupted
                )
            })
            .filter_map(|entry| entry.undoes.map(|original| (original, entry.id)))
            .collect();
        for (original, undo_id) in links {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == original) {
                entry.undone_by = Some(undo_id);
            }
        }
    }

    fn reconcile_interrupted(&mut self) -> std::io::Result<()> {
        let pending: Vec<JournalEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.outcome == JournalOutcome::Pending)
            .cloned()
            .collect();
        for mut entry in pending {
            entry.outcome = JournalOutcome::Interrupted;
            entry.failure = Some(
                "Droidsmith stopped before the device operation recorded a terminal outcome; verify device state before retrying"
                    .to_string(),
            );
            self.replace(entry)?;
        }
        Ok(())
    }

    fn persist_line(&self, entry: &JournalEntry) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_persist == Some(PersistFailure::Open) {
            return Err(injected("open"));
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        #[cfg(test)]
        if self.fail_persist == Some(PersistFailure::PartialWrite) {
            f.write_all(&line.as_bytes()[..line.len() / 2])?;
            f.flush()?;
            f.sync_data()?;
            return Err(injected("partial write"));
        }
        #[cfg(test)]
        if self.fail_persist == Some(PersistFailure::Write) {
            return Err(injected("write"));
        }
        writeln!(f, "{line}")?;
        #[cfg(test)]
        if self.fail_persist == Some(PersistFailure::Flush) {
            return Err(injected("flush"));
        }
        f.flush()?;
        #[cfg(test)]
        if self.fail_persist == Some(PersistFailure::Sync) {
            return Err(injected("sync"));
        }
        f.sync_data()?;
        Ok(())
    }
}

fn invalid_transition(id: u64, outcome: JournalOutcome) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("journal entry {id} cannot transition from {outcome:?}"),
    )
}

fn same_plan(left: &PlannedAction, right: &PlannedAction) -> bool {
    left.incident_id == right.incident_id
        && left.before_state == right.before_state
        && left.args == right.args
        && left.request.serial == right.request.serial
        && left.request.target == right.request.target
        && left.request.package == right.request.package
        && left.request.kind == right.request.kind
        && left.request.user_id == right.request.user_id
        && left.request.pack_context == right.request.pack_context
        && left.request.context == right.request.context
}

/// Every production record ends in `\n`. If a process or filesystem failure
/// leaves an incomplete tail, discard only that tail before replay/appending.
/// A truncate/sync failure aborts journal open, so no mutation can proceed on
/// top of an unrepaired log.
fn repair_partial_tail(path: &Path) -> std::io::Result<()> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return Ok(());
    }
    let keep = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(keep as u64)?;
    file.sync_data()
}

#[cfg(test)]
fn injected(point: &str) -> std::io::Error {
    std::io::Error::other(format!("injected journal {point} failure"))
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
    let original_kind = entry.applied.plan.request.kind;
    match original_kind {
        crate::adb::actions::ActionKind::GrantPermission
            if entry.applied.before_state != "revoked"
                || entry.applied.after_state != "granted" =>
        {
            return None;
        }
        crate::adb::actions::ActionKind::RevokePermission
            if entry.applied.before_state != "granted"
                || entry.applied.after_state != "revoked" =>
        {
            return None;
        }
        _ => {}
    }
    let kind = original_kind.inverse()?;
    Some(ActionRequest {
        serial: entry.applied.plan.request.serial.clone(),
        target: entry.applied.plan.request.target.clone(),
        package: entry.applied.plan.request.package.clone(),
        kind,
        // Undo must target the exact same Android user the original
        // action mutated, or a work-profile disable would re-enable on
        // the owner instead.
        user_id: entry.applied.plan.request.user_id,
        pack_context: entry.applied.plan.request.pack_context.clone(),
        context: crate::adb::actions::ActionContext {
            confirmation_source: crate::adb::actions::ConfirmationSource::JournalUndo,
            ..entry.applied.plan.request.context.clone()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::actions::{plan, ActionKind, ActionRequest, PlannedAction};
    use std::sync::Mutex;

    static TMP_COUNTER: Mutex<u32> = Mutex::new(0);

    fn target(serial: &str) -> crate::adb::DeviceTarget {
        crate::adb::DeviceTarget {
            serial: serial.into(),
            transport_id: Some(1),
            connection_generation: 2,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

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
                target: target(serial),
                package: pkg.into(),
                kind,
                user_id: 0,
                pack_context: None,
                context: crate::adb::actions::ActionContext::default(),
            }),
            stdout: "Package x new state: disabled\n".into(),
            display_stdout: "Package x new state: disabled\n".into(),
            before_state: "installed_enabled".into(),
            after_state: "installed_disabled".into(),
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
    fn pack_provenance_and_override_round_trip() {
        let dir = fresh_tmp_dir("pack-context");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let mut applied = fake_applied("abc", "com.foo", ActionKind::Disable);
        applied.plan.request.pack_context = Some(crate::adb::actions::PackActionContext {
            pack_id: "pixel-stock".into(),
            revision: 4,
            provenance_source: "https://example.invalid/pixel".into(),
            provenance_license: "MIT".into(),
            compatibility_status: "mismatch".into(),
            override_accepted: true,
        });
        journal.record(applied).unwrap();
        drop(journal);

        let reloaded = Journal::open(&dir, "abc").unwrap();
        let context = reloaded.entries()[0]
            .applied
            .plan
            .request
            .pack_context
            .as_ref()
            .unwrap();
        assert_eq!(context.pack_id, "pixel-stock");
        assert_eq!(context.revision, 4);
        assert!(context.override_accepted);
    }

    #[test]
    fn unsafe_transport_override_round_trips_in_mutation_audit() {
        let dir = fresh_tmp_dir("transport-override");
        let mut journal = Journal::open(&dir, "wifi.local:5555").unwrap();
        let mut applied = fake_applied("wifi.local:5555", "com.foo", ActionKind::Disable);
        applied.plan.request.target.transport_kind = crate::adb::DeviceTransportKind::LegacyTcp;
        applied.plan.request.target.untrusted_transport_override = true;
        applied.plan.request.context.transport_override =
            Some(crate::adb::DeviceTransportKind::LegacyTcp);
        journal.record(applied).unwrap();
        drop(journal);

        let reloaded = Journal::open(&dir, "wifi.local:5555").unwrap();
        assert_eq!(
            reloaded.entries()[0]
                .applied
                .plan
                .request
                .context
                .transport_override,
            Some(crate::adb::DeviceTransportKind::LegacyTcp)
        );
    }

    #[test]
    fn concurrent_records_produce_unique_ids() {
        // Regression for IMP-30: independent `Journal::open` instances
        // used to derive the same next_id and append duplicate ids under
        // concurrency. `with_journal` serializes the open→record cycle.
        let dir = fresh_tmp_dir("concurrent");
        std::fs::create_dir_all(&dir).unwrap();
        const THREADS: usize = 16;

        std::thread::scope(|scope| {
            for i in 0..THREADS {
                let dir = dir.clone();
                scope.spawn(move || {
                    let pkg = format!("com.example.p{i}");
                    with_journal(&dir, "race", |journal| {
                        journal.record(fake_applied("race", &pkg, ActionKind::Disable))?;
                        Ok::<_, std::io::Error>(())
                    })
                    .unwrap();
                });
            }
        });

        let reloaded = Journal::open(&dir, "race").unwrap();
        assert_eq!(reloaded.entries().len(), THREADS);
        let mut ids: Vec<u64> = reloaded.entries().iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), THREADS, "all journal ids must be unique");
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
    fn permission_undo_requires_a_verified_state_transition() {
        let dir = fresh_tmp_dir("permission-undo");
        let make_applied = |before: &str, after: &str| AppliedAction {
            plan: plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: "com.foo".into(),
                kind: ActionKind::GrantPermission,
                user_id: 0,
                pack_context: None,
                context: crate::adb::actions::ActionContext {
                    confirmation_source: crate::adb::actions::ConfirmationSource::PermissionToggle,
                    permission: Some("android.permission.CAMERA".into()),
                    shell_argv: Vec::new(),
                    transport_override: None,
                },
            }),
            stdout: String::new(),
            display_stdout: String::new(),
            before_state: before.into(),
            after_state: after.into(),
            applied_at: "2026-07-14T12:00:00Z".into(),
        };

        let mut journal = Journal::open(&dir, "abc").unwrap();
        journal.record(make_applied("unknown", "granted")).unwrap();
        assert!(undo_request_for(&journal, 1).is_none());
        journal.record(make_applied("revoked", "granted")).unwrap();
        let undo = undo_request_for(&journal, 2).unwrap();
        assert_eq!(undo.kind, ActionKind::RevokePermission);
        assert_eq!(
            undo.context.permission.as_deref(),
            Some("android.permission.CAMERA")
        );
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
            target: target("abc"),
            package: "com.foo".into(),
            kind: ActionKind::Disable,
            user_id: 0,
            pack_context: None,
            context: crate::adb::actions::ActionContext::default(),
        });
        let entry = JournalEntry {
            id: 1,
            applied: AppliedAction {
                plan: good,
                stdout: "ok".into(),
                display_stdout: "ok".into(),
                before_state: "unknown".into(),
                after_state: "unknown".into(),
                applied_at: "2026-05-25T12:00:00Z".into(),
            },
            undone_by: None,
            undoes: None,
            outcome: JournalOutcome::Succeeded,
            failure: None,
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

    #[test]
    fn execute_syncs_pending_intent_before_running_operation() {
        let dir = fresh_tmp_dir("wal-order");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let applied = fake_applied("abc", "com.foo", ActionKind::Disable);
        let path = journal.path().to_path_buf();
        let result = journal
            .execute(applied.plan.clone(), None, "2026-05-25T11:59:59Z", |plan| {
                let first = fs::read_to_string(&path).unwrap();
                let intent: JournalEntry =
                    serde_json::from_str(first.lines().next().unwrap()).unwrap();
                assert_eq!(intent.outcome, JournalOutcome::Pending);
                assert_eq!(intent.applied.plan.args, plan.args);
                Ok::<_, &'static str>(applied)
            })
            .unwrap();
        assert_eq!(result.outcome, JournalOutcome::Succeeded);
    }

    #[test]
    fn persistence_failures_never_run_the_device_operation() {
        for (index, failure) in [
            PersistFailure::Open,
            PersistFailure::Write,
            PersistFailure::Flush,
            PersistFailure::Sync,
            PersistFailure::PartialWrite,
        ]
        .into_iter()
        .enumerate()
        {
            let dir = fresh_tmp_dir(&format!("wal-failure-{index}"));
            let mut journal = Journal::open(&dir, "abc").unwrap();
            journal.fail_persist = Some(failure);
            let ran = std::cell::Cell::new(false);
            let applied = fake_applied("abc", "com.foo", ActionKind::Disable);
            let result =
                journal.execute(applied.plan.clone(), None, "2026-05-25T11:59:59Z", |_| {
                    ran.set(true);
                    Ok::<_, &'static str>(applied)
                });
            assert!(matches!(result, Err(ExecuteError::Journal(_))));
            assert!(!ran.get(), "device operation ran after {failure:?} failure");
        }
    }

    #[test]
    fn startup_marks_a_crashed_operation_interrupted() {
        let dir = fresh_tmp_dir("crash-recovery");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let applied = fake_applied("abc", "com.foo", ActionKind::Disable);
        let id = journal
            .begin(applied.plan, "2026-05-25T11:59:59Z", None)
            .unwrap();
        drop(journal); // process stopped after durable intent, before outcome

        let recovered = Journal::open(&dir, "abc").unwrap();
        let entry = recovered.entry(id).unwrap();
        assert_eq!(entry.outcome, JournalOutcome::Interrupted);
        assert!(entry
            .failure
            .as_deref()
            .unwrap()
            .contains("verify device state"));
    }

    #[test]
    fn failed_terminal_sync_recovers_as_visible_interrupted_operation() {
        let dir = fresh_tmp_dir("terminal-sync-failure");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let applied = fake_applied("abc", "com.foo", ActionKind::Disable);
        let id = journal
            .begin(applied.plan.clone(), "2026-05-25T11:59:59Z", None)
            .unwrap();
        journal.fail_persist = Some(PersistFailure::Sync);
        assert!(journal.succeed(id, applied).is_err());
        drop(journal);

        let recovered = Journal::open(&dir, "abc").unwrap();
        assert!(matches!(
            recovered.entry(id).unwrap().outcome,
            JournalOutcome::Succeeded | JournalOutcome::Interrupted
        ));
    }

    #[test]
    fn operation_errors_receive_a_durable_failed_outcome() {
        let dir = fresh_tmp_dir("terminal-operation-failure");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let applied = fake_applied("abc", "com.foo", ActionKind::Disable);
        let result = journal.execute(applied.plan, None, "2026-05-25T11:59:59Z", |_| {
            Err::<AppliedAction, _>("adb rejected the mutation")
        });
        assert!(matches!(result, Err(ExecuteError::Operation(_))));
        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.entries()[0].outcome, JournalOutcome::Failed);
        assert_eq!(
            journal.entries()[0].failure.as_deref(),
            Some("adb rejected the mutation")
        );
        drop(journal);
        assert_eq!(
            Journal::open(&dir, "abc").unwrap().entries()[0].outcome,
            JournalOutcome::Failed
        );
    }

    #[test]
    fn shell_failures_persist_only_a_redacted_summary() {
        let dir = fresh_tmp_dir("shell-failure-redaction");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let plan = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: String::new(),
            kind: ActionKind::Shell,
            user_id: 0,
            pack_context: None,
            context: crate::adb::actions::ActionContext {
                confirmation_source: crate::adb::actions::ConfirmationSource::ConsoleReview,
                permission: None,
                shell_argv: vec!["rm".into(), "/sdcard/private.txt".into()],
                transport_override: None,
            },
        });
        let result = journal.execute(plan, None, "2026-07-14T12:00:00Z", |_| {
            Err::<AppliedAction, _>("secret device failure from abc")
        });
        assert!(matches!(result, Err(ExecuteError::Operation(_))));
        let failure = journal.entries()[0].failure.as_deref().unwrap();
        assert!(failure.contains("redacted"));
        assert!(!failure.contains("secret device failure"));
    }

    #[test]
    fn truncated_terminal_record_recovers_the_durable_intent() {
        let dir = fresh_tmp_dir("truncated-outcome");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        let applied = fake_applied("abc", "com.foo", ActionKind::Disable);
        let id = journal
            .begin(applied.plan.clone(), "2026-05-25T11:59:59Z", None)
            .unwrap();
        let mut terminal = journal.entry(id).unwrap().clone();
        terminal.outcome = JournalOutcome::Succeeded;
        terminal.applied = applied;
        let encoded = serde_json::to_string(&terminal).unwrap();
        let path = journal.path().to_path_buf();
        drop(journal);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&encoded.as_bytes()[..encoded.len() / 2])
            .unwrap();
        file.flush().unwrap();
        file.sync_data().unwrap();
        drop(file);

        let recovered = Journal::open(&dir, "abc").unwrap();
        assert_eq!(
            recovered.entry(id).unwrap().outcome,
            JournalOutcome::Interrupted
        );
        assert!(fs::read(&path).unwrap().ends_with(b"\n"));
    }

    #[test]
    fn interrupted_undo_is_idempotently_blocked_after_restart() {
        let dir = fresh_tmp_dir("undo-interrupted");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        journal
            .record(fake_applied("abc", "com.foo", ActionKind::Disable))
            .unwrap();
        let inverse = fake_applied("abc", "com.foo", ActionKind::Enable);
        let undo_id = journal
            .begin(inverse.plan.clone(), "2026-05-25T12:01:00Z", Some(1))
            .unwrap();
        assert!(undo_request_for(&journal, 1).is_none());
        drop(journal);

        let mut recovered = Journal::open(&dir, "abc").unwrap();
        assert_eq!(
            recovered.entry(undo_id).unwrap().outcome,
            JournalOutcome::Interrupted
        );
        assert_eq!(recovered.entry(1).unwrap().undone_by, Some(undo_id));
        assert!(recovered
            .begin(inverse.plan, "2026-05-25T12:02:00Z", Some(1))
            .is_err());
    }

    #[test]
    fn failed_undo_releases_original_and_success_is_idempotent() {
        let dir = fresh_tmp_dir("undo-terminal");
        let mut journal = Journal::open(&dir, "abc").unwrap();
        journal
            .record(fake_applied("abc", "com.foo", ActionKind::Disable))
            .unwrap();
        let inverse = fake_applied("abc", "com.foo", ActionKind::Enable);
        let failed_id = journal
            .begin(inverse.plan.clone(), "2026-05-25T12:01:00Z", Some(1))
            .unwrap();
        journal
            .fail(failed_id, "adb failed".into(), "2026-05-25T12:01:01Z")
            .unwrap();
        assert_eq!(journal.entry(1).unwrap().undone_by, None);
        assert!(undo_request_for(&journal, 1).is_some());

        let success_id = journal
            .begin(inverse.plan.clone(), "2026-05-25T12:02:00Z", Some(1))
            .unwrap();
        journal.succeed(success_id, inverse.clone()).unwrap();
        let line_count = fs::read_to_string(journal.path()).unwrap().lines().count();
        journal.succeed(success_id, inverse).unwrap();
        assert_eq!(
            fs::read_to_string(journal.path()).unwrap().lines().count(),
            line_count,
            "repeating success must not append another transition"
        );
        assert_eq!(journal.entry(1).unwrap().undone_by, Some(success_id));
    }

    #[test]
    fn legacy_success_rows_default_to_succeeded() {
        let dir = fresh_tmp_dir("legacy-outcome");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.jsonl");
        let mut value = serde_json::to_value(JournalEntry {
            id: 1,
            applied: fake_applied("abc", "com.foo", ActionKind::Disable),
            undone_by: None,
            undoes: None,
            outcome: JournalOutcome::Succeeded,
            failure: None,
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("outcome");
        value.as_object_mut().unwrap().remove("failure");
        let mut file = File::create(path).unwrap();
        writeln!(file, "{}", serde_json::to_string(&value).unwrap()).unwrap();
        file.sync_data().unwrap();

        let journal = Journal::open(&dir, "abc").unwrap();
        assert_eq!(journal.entry(1).unwrap().outcome, JournalOutcome::Succeeded);
    }
}
