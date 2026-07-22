//! Canonical, shell-interpolation-free plans for device-side file mutations.

use serde::{Deserialize, Serialize};

use crate::adb::{actions, AdbTransport, DeviceTarget, TransportError};

const MAX_REMOTE_PATH_BYTES: usize = 4_096;

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteFileMutationKind {
    Mkdir,
    Rename,
    DeleteFile,
    DeleteDirectory,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFileMutationRequest {
    pub kind: RemoteFileMutationKind,
    pub source_path: String,
    #[serde(default)]
    pub destination_path: Option<String>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteFileMutationPlan {
    pub kind: RemoteFileMutationKind,
    pub source_path: String,
    pub destination_path: Option<String>,
    pub argv: Vec<String>,
    pub description: String,
    pub destructive: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RemoteFileError {
    #[error("device path cannot be empty")]
    EmptyPath,
    #[error("device path must be absolute: {0:?}")]
    RelativePath(String),
    #[error("device path is too long")]
    PathTooLong,
    #[error("device path must be normalized and contain no control characters")]
    UnsafePath,
    #[error("refusing to mutate protected device path {0:?}")]
    ProtectedPath(String),
    #[error("rename requires a destination in the same directory")]
    RenameDestination,
    #[error("{0:?} does not accept a destination path")]
    UnexpectedDestination(RemoteFileMutationKind),
}

pub fn validate_path(value: &str) -> Result<String, RemoteFileError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RemoteFileError::EmptyPath);
    }
    if !trimmed.starts_with('/') || trimmed.starts_with('-') {
        return Err(RemoteFileError::RelativePath(trimmed.to_string()));
    }
    if trimmed.len() > MAX_REMOTE_PATH_BYTES {
        return Err(RemoteFileError::PathTooLong);
    }
    if trimmed.chars().any(char::is_control)
        || trimmed.split('/').any(|part| matches!(part, "." | ".."))
        || (trimmed.len() > 1 && trimmed.ends_with('/'))
        || trimmed.contains("//")
    {
        return Err(RemoteFileError::UnsafePath);
    }
    Ok(trimmed.to_string())
}

fn validate_mutation_path(value: &str) -> Result<String, RemoteFileError> {
    let path = validate_path(value)?;
    if matches!(
        path.as_str(),
        "/" | "/sdcard"
            | "/storage"
            | "/storage/emulated"
            | "/data"
            | "/system"
            | "/system_ext"
            | "/product"
            | "/vendor"
            | "/apex"
    ) {
        return Err(RemoteFileError::ProtectedPath(path));
    }
    Ok(path)
}

pub fn plan(
    request: &RemoteFileMutationRequest,
) -> Result<RemoteFileMutationPlan, RemoteFileError> {
    let source_path = validate_mutation_path(&request.source_path)?;
    let (destination_path, argv, description, destructive) = match request.kind {
        RemoteFileMutationKind::Mkdir => {
            if request.destination_path.is_some() {
                return Err(RemoteFileError::UnexpectedDestination(request.kind));
            }
            (
                None,
                vec!["mkdir".to_string(), source_path.clone()],
                format!("Create device directory {source_path:?}"),
                false,
            )
        }
        RemoteFileMutationKind::Rename => {
            let destination = request
                .destination_path
                .as_deref()
                .ok_or(RemoteFileError::RenameDestination)
                .and_then(validate_mutation_path)?;
            if source_path == destination
                || parent(&source_path).is_none()
                || parent(&source_path) != parent(&destination)
            {
                return Err(RemoteFileError::RenameDestination);
            }
            (
                Some(destination.clone()),
                vec![
                    "mv".to_string(),
                    "-n".to_string(),
                    source_path.clone(),
                    destination.clone(),
                ],
                format!("Rename device path {source_path:?} to {destination:?}"),
                true,
            )
        }
        RemoteFileMutationKind::DeleteFile => {
            if request.destination_path.is_some() {
                return Err(RemoteFileError::UnexpectedDestination(request.kind));
            }
            (
                None,
                vec!["rm".to_string(), "-f".to_string(), source_path.clone()],
                format!("Permanently delete device file {source_path:?}"),
                true,
            )
        }
        RemoteFileMutationKind::DeleteDirectory => {
            if request.destination_path.is_some() {
                return Err(RemoteFileError::UnexpectedDestination(request.kind));
            }
            (
                None,
                vec!["rm".to_string(), "-rf".to_string(), source_path.clone()],
                format!("Permanently delete device directory {source_path:?} and its contents"),
                true,
            )
        }
    };
    Ok(RemoteFileMutationPlan {
        kind: request.kind,
        source_path,
        destination_path,
        argv,
        description,
        destructive,
    })
}

pub fn action_plan(
    target: DeviceTarget,
    user_id: u32,
    transport_override: Option<crate::adb::DeviceTransportKind>,
    plan: &RemoteFileMutationPlan,
) -> actions::PlannedAction {
    let mut action = actions::plan(actions::ActionRequest {
        serial: target.serial.clone(),
        target,
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::FileManagerReview,
            permission: None,
            shell_argv: plan.argv.clone(),
            device_control_restore_argv: Vec::new(),
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    action.description = plan.description.clone();
    action
}

pub fn capture_state(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
    argv: &[String],
) -> String {
    let paths = match argv {
        [command, path] if command == "mkdir" => vec![path.as_str()],
        [command, _, path] if command == "rm" => vec![path.as_str()],
        [command, option, source, destination] if command == "mv" && option == "-n" => {
            vec![source.as_str(), destination.as_str()]
        }
        _ => return "not_captured".to_string(),
    };
    paths
        .into_iter()
        .map(|path| {
            let state = capture_path_state(transport, target, path);
            format!("{path}={state}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn capture_path_state(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
    path: &str,
) -> &'static str {
    if transport.shell_target(target, &["ls", "-ld", path]).is_ok() {
        "present"
    } else {
        "absent_or_inaccessible"
    }
}

/// Prove the requested filesystem state after a successful shell exit. This
/// catches commands such as `mv -n` that can leave the source untouched while
/// still returning success when the destination already exists.
pub fn verify_transition(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
    argv: &[String],
) -> Result<(), TransportError> {
    let verified = match argv {
        [command, path] if command == "mkdir" => path_exists(transport, target, path)?,
        [command, _, path] if command == "rm" => !path_exists(transport, target, path)?,
        [command, option, source, destination] if command == "mv" && option == "-n" => {
            !path_exists(transport, target, source)? && path_exists(transport, target, destination)?
        }
        _ => false,
    };
    if verified {
        Ok(())
    } else {
        Err(TransportError::Parse(
            "remote file command exited successfully but the reviewed state change was not observed"
                .to_string(),
        ))
    }
}

fn path_exists(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
    path: &str,
) -> Result<bool, TransportError> {
    match transport.shell_target(target, &["test", "-e", path]) {
        Ok(_) => Ok(true),
        Err(TransportError::Exit { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

fn parent(path: &str) -> Option<&str> {
    let index = path.rfind('/')?;
    Some(if index == 0 { "/" } else { &path[..index] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::{transport::MockTransport, DeviceTransportKind};

    fn target() -> DeviceTarget {
        DeviceTarget {
            serial: "abc".to_string(),
            transport_id: Some(1),
            connection_generation: 1,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".to_string()),
            transport_kind: DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    #[test]
    fn plans_unicode_and_spaces_as_distinct_argv_values() {
        let plan = plan(&RemoteFileMutationRequest {
            kind: RemoteFileMutationKind::Rename,
            source_path: "/sdcard/My files/старое имя.txt".to_string(),
            destination_path: Some("/sdcard/My files/новое имя.txt".to_string()),
        })
        .unwrap();
        assert_eq!(
            plan.argv,
            [
                "mv",
                "-n",
                "/sdcard/My files/старое имя.txt",
                "/sdcard/My files/новое имя.txt"
            ]
        );
    }

    #[test]
    fn rejects_traversal_protected_roots_and_cross_directory_rename() {
        for path in ["/sdcard/../data", "/", "/sdcard", "/storage/emulated"] {
            assert!(plan(&RemoteFileMutationRequest {
                kind: RemoteFileMutationKind::DeleteDirectory,
                source_path: path.to_string(),
                destination_path: None,
            })
            .is_err());
        }
        assert_eq!(
            plan(&RemoteFileMutationRequest {
                kind: RemoteFileMutationKind::Rename,
                source_path: "/sdcard/one.txt".to_string(),
                destination_path: Some("/sdcard/Download/one.txt".to_string()),
            })
            .unwrap_err(),
            RemoteFileError::RenameDestination
        );
    }

    #[test]
    fn delete_directory_is_an_explicit_recursive_plan() {
        let plan = plan(&RemoteFileMutationRequest {
            kind: RemoteFileMutationKind::DeleteDirectory,
            source_path: "/sdcard/Folder".to_string(),
            destination_path: None,
        })
        .unwrap();
        assert!(plan.destructive);
        assert_eq!(plan.argv, ["rm", "-rf", "/sdcard/Folder"]);
    }

    #[test]
    fn rename_verification_rejects_a_successful_no_op() {
        let argv = vec![
            "mv".to_string(),
            "-n".to_string(),
            "/sdcard/source.txt".to_string(),
            "/sdcard/target.txt".to_string(),
        ];
        let failed = MockTransport::new();
        failed.expect_shell(
            "abc",
            &["test", "-e", "/sdcard/source.txt"],
            Ok(String::new()),
        );
        assert!(verify_transition(&failed, &target(), &argv).is_err());

        let verified = MockTransport::new();
        verified.expect_shell(
            "abc",
            &["test", "-e", "/sdcard/source.txt"],
            Err(TransportError::Exit {
                code: 1,
                stderr: String::new(),
            }),
        );
        verified.expect_shell(
            "abc",
            &["test", "-e", "/sdcard/target.txt"],
            Ok(String::new()),
        );
        assert!(verify_transition(&verified, &target(), &argv).is_ok());
    }
}
