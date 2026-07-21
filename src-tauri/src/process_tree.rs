//! Cross-platform child-process containment for Android tooling.
//!
//! Every external tool starts without a Windows console and in an isolated
//! process group. Cancellation and timeout paths terminate the whole group so
//! helpers spawned by adb/fastboot/scrcpy cannot outlive the operation.

use std::io;
use std::process::{Child, Command, ExitStatus};

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn configure(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

pub(crate) fn terminate(child: &mut Child) -> io::Result<ExitStatus> {
    if let Some(status) = child.try_wait()? {
        return Ok(status);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;

        let status = Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        if !status.is_ok_and(|status| status.success()) {
            if let Some(status) = kill_direct_if_running(child)? {
                return Ok(status);
            }
        }
        child.wait()
    }

    #[cfg(unix)]
    {
        let process_group = i32::try_from(child.id()).ok().map(|pid| -pid);
        #[allow(unsafe_code)]
        let killed_group = process_group.is_some_and(|group| unsafe {
            // `configure` made the child its process-group leader. A negative
            // pid therefore targets only this operation's full descendant set.
            libc::kill(group, libc::SIGKILL) == 0
        });
        if !killed_group {
            if let Some(status) = kill_direct_if_running(child)? {
                return Ok(status);
            }
        }
        child.wait()
    }

    #[cfg(not(any(windows, unix)))]
    {
        if let Some(status) = kill_direct_if_running(child)? {
            return Ok(status);
        }
        child.wait()
    }
}

fn kill_direct_if_running(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    if let Some(status) = child.try_wait()? {
        return Ok(Some(status));
    }

    match child.kill() {
        Ok(()) => Ok(None),
        Err(error) => match child.try_wait()? {
            Some(status) => Ok(Some(status)),
            None => Err(error),
        },
    }
}

#[cfg(all(test, any(windows, unix)))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    fn silent(mut command: Command) -> Command {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    #[cfg(unix)]
    fn long_running_command() -> Command {
        let mut command = Command::new("sleep");
        command.arg("30");
        silent(command)
    }

    #[cfg(unix)]
    fn quick_command() -> Command {
        silent(Command::new("true"))
    }

    #[cfg(windows)]
    fn long_running_command() -> Command {
        let mut command = Command::new("cmd");
        command.args(["/C", "ping", "-n", "30", "127.0.0.1"]);
        silent(command)
    }

    #[cfg(windows)]
    fn quick_command() -> Command {
        let mut command = Command::new("cmd");
        command.args(["/C", "exit", "0"]);
        silent(command)
    }

    #[test]
    fn configure_spawns_into_an_isolated_group_and_runs() {
        // A configured command must still spawn and run normally; the isolation
        // flags only affect signal/console behaviour, not execution.
        let mut command = quick_command();
        configure(&mut command);
        let mut child = command.spawn().expect("spawn configured child");
        let status = child.wait().expect("wait for quick child");
        assert!(status.success());
    }

    #[test]
    fn terminate_kills_a_running_child_and_reaps_it() {
        let mut command = long_running_command();
        configure(&mut command);
        let mut child = command.spawn().expect("spawn long-running child");
        // The child is still running before we terminate it.
        assert!(child.try_wait().expect("try_wait").is_none());

        let started = Instant::now();
        let status = terminate(&mut child).expect("terminate running child");
        // A force-killed helper never exits successfully, and the group kill
        // must return promptly (it reaps via wait() internally).
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn terminate_returns_the_status_of_an_already_exited_child() {
        let mut command = quick_command();
        configure(&mut command);
        let mut child = command.spawn().expect("spawn quick child");

        // Let the child exit on its own (a bare `true` / `cmd /C exit 0`
        // finishes in milliseconds) without reaping it, so terminate takes its
        // early try_wait() path and reports the real success status.
        sleep(Duration::from_millis(400));
        let status = terminate(&mut child).expect("terminate exited child");
        assert!(status.success());
    }

    #[test]
    fn kill_direct_reports_status_for_an_already_exited_child() {
        let mut command = quick_command();
        configure(&mut command);
        let mut child = command.spawn().expect("spawn quick child");
        // The direct-kill fallback (used when a group kill fails) must surface
        // an already-exited child's real status instead of signalling a dead
        // pid and returning an error.
        sleep(Duration::from_millis(400));
        let status = kill_direct_if_running(&mut child)
            .expect("direct kill on exited child")
            .expect("exited child yields a status");
        assert!(status.success());
    }

    #[test]
    fn kill_direct_signals_a_running_child_then_reaps() {
        let mut command = long_running_command();
        configure(&mut command);
        let mut child = command.spawn().expect("spawn long-running child");
        // A live child returns None (the kill signal was issued but the child
        // is not yet reaped); the caller then reaps it via wait(). This is the
        // fallback branch taken when the OS group kill did not succeed.
        let started = Instant::now();
        let outcome =
            kill_direct_if_running(&mut child).expect("direct kill on running child");
        assert!(outcome.is_none());
        let status = child.wait().expect("reap direct-killed child");
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}
