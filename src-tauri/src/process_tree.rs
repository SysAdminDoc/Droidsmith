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
