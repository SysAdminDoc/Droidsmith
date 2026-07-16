//! Typed release gate for pack, quirk, and profile YAML inputs.
//!
//! The caller must provide at least one file of every kind. Validation is
//! read-only and uses the same bounded loaders as the application.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use droidsmith_lib::{contribution_schema, packs, profile, quirks};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SchemaKind {
    Pack,
    Quirk,
    Profile,
}

impl SchemaKind {
    const fn flag(self) -> &'static str {
        match self {
            Self::Pack => "--pack",
            Self::Quirk => "--quirk",
            Self::Profile => "--profile",
        }
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut inputs = Vec::new();
    let mut generated_action = None;
    while let Some(flag) = args.next() {
        if matches!(flag.as_str(), "--check-generated" | "--write-generated") {
            if generated_action.is_some() {
                return usage("only one generated-schema action may be supplied");
            }
            let Some(root) = args.next() else {
                return usage(&format!("{flag} requires the repository root"));
            };
            generated_action = Some((flag, PathBuf::from(root)));
            continue;
        }
        let kind = match flag.as_str() {
            "--pack" => SchemaKind::Pack,
            "--quirk" => SchemaKind::Quirk,
            "--profile" => SchemaKind::Profile,
            _ => return usage(&format!("unknown schema flag {flag:?}")),
        };
        let Some(path) = args.next() else {
            return usage(&format!("{} requires a file path", kind.flag()));
        };
        inputs.push((kind, PathBuf::from(path)));
    }

    if let Some((action, root)) = generated_action.as_ref() {
        let result = if action == "--check-generated" {
            contribution_schema::check_generated(root)
        } else {
            contribution_schema::write_generated(root)
        };
        match result {
            Ok(paths) => {
                for path in paths {
                    println!("[ok]    {}", path.display());
                }
            }
            Err(error) => {
                eprintln!("[ERROR] generated schemas: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    if inputs.is_empty() {
        if generated_action.is_some() {
            return ExitCode::SUCCESS;
        }
        return usage("no schema inputs or generated-schema action supplied");
    }

    for kind in [SchemaKind::Pack, SchemaKind::Quirk, SchemaKind::Profile] {
        if !inputs.iter().any(|(candidate, _)| *candidate == kind) {
            return usage(&format!("at least one {} input is required", kind.flag()));
        }
    }

    let mut failed = false;
    for (kind, path) in inputs {
        match validate(kind, &path) {
            Ok(count) => println!("[ok]    {} ({count} entries)", path.display()),
            Err(error) => {
                eprintln!("[ERROR] {}: {error}", path.display());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        println!("Schema validation OK");
        ExitCode::SUCCESS
    }
}

fn validate(kind: SchemaKind, path: &Path) -> Result<usize, String> {
    match kind {
        SchemaKind::Pack => packs::load(path)
            .map(|pack| pack.packages.len())
            .map_err(|error| error.to_string()),
        SchemaKind::Quirk => quirks::load_file(path)
            .map(|entries| entries.len())
            .map_err(|error| error.to_string()),
        SchemaKind::Profile => profile::inspect(path)
            .map(|document| match document {
                profile::ProfileDocument::Current { profile } => profile.actions.len(),
                profile::ProfileDocument::MigrationAvailable { migration } => {
                    migration.profile.actions.len()
                }
            })
            .map_err(|error| error.to_string()),
    }
}

fn usage(error: &str) -> ExitCode {
    eprintln!("error: {error}");
    eprintln!(
        "usage: droidsmith-schema-lint [--check-generated|--write-generated] <repo-root> [--pack <file>... --quirk <file>... --profile <file>...]"
    );
    ExitCode::from(2)
}
