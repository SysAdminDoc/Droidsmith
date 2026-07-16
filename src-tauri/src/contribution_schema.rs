//! Deterministic JSON Schemas and compatibility policy for contributed YAML.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{packs, profile, quirks};

const POLICY_VERSION: u32 = 1;
const POLICY_PATH: &str = "contribution-schema-policy.json";

struct GeneratedSchema {
    kind: &'static str,
    document_version: &'static str,
    relative_path: &'static str,
    migration: &'static str,
    compatibility_fixture: &'static str,
    fixture_version: &'static str,
    contents: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaPolicy {
    policy_version: u32,
    schemas: BTreeMap<String, SchemaPolicyEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaPolicyEntry {
    document_version: String,
    schema_sha256: String,
    migration: String,
    compatibility_fixture: String,
    fixture_version: String,
}

/// Write deterministic schemas without changing the compatibility policy.
///
/// Keeping policy updates manual is intentional: a changed fingerprint must be
/// paired with a document-version bump, migration note, and fixture review.
pub fn write_generated(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let documents = generated_schemas()?;
    let mut written = Vec::with_capacity(documents.len());
    for document in documents {
        let path = repo_root.join(document.relative_path);
        let parent = path
            .parent()
            .ok_or_else(|| format!("schema path {} has no parent", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        fs::write(&path, document.contents)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// Fail when generated schemas, schema fingerprints, migration notes, or
/// compatibility fixtures drift from the checked-in contribution contract.
pub fn check_generated(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let documents = generated_schemas()?;
    let policy_path = repo_root.join(POLICY_PATH);
    let policy_text = fs::read_to_string(&policy_path)
        .map_err(|error| format!("could not read {}: {error}", policy_path.display()))?;
    let policy: SchemaPolicy = serde_json::from_str(&policy_text)
        .map_err(|error| format!("could not parse {}: {error}", policy_path.display()))?;
    if policy.policy_version != POLICY_VERSION {
        return Err(format!(
            "unsupported contribution schema policy version {} (expected {POLICY_VERSION})",
            policy.policy_version
        ));
    }
    if policy.schemas.len() != documents.len() {
        return Err(format!(
            "schema policy lists {} schemas but {} are generated",
            policy.schemas.len(),
            documents.len()
        ));
    }

    let mut checked = Vec::with_capacity(documents.len());
    for document in documents {
        let path = repo_root.join(document.relative_path);
        let actual = fs::read_to_string(&path).map_err(|error| {
            format!(
                "could not read generated schema {}: {error}",
                path.display()
            )
        })?;
        if actual != document.contents {
            return Err(format!(
                "{} is stale; run droidsmith-schema-lint --write-generated <repo-root>",
                path.display()
            ));
        }

        let entry = policy
            .schemas
            .get(document.kind)
            .ok_or_else(|| format!("schema policy has no {:?} entry", document.kind))?;
        let fingerprint = sha256_hex(document.contents.as_bytes());
        if entry.document_version != document.document_version || entry.schema_sha256 != fingerprint
        {
            return Err(format!(
                "{} schema changed without a matching document-version policy update; breaking changes require a version bump, migration note, and compatibility fixture (generated version {}, sha256 {})",
                document.kind, document.document_version, fingerprint
            ));
        }
        if entry.migration != document.migration || entry.migration.trim().is_empty() {
            return Err(format!(
                "{} schema policy migration note does not match the runtime migration path",
                document.kind
            ));
        }
        if entry.compatibility_fixture != document.compatibility_fixture
            || entry.fixture_version != document.fixture_version
        {
            return Err(format!(
                "{} schema policy compatibility fixture metadata is stale",
                document.kind
            ));
        }
        validate_fixture(repo_root, &document)?;
        checked.push(path);
    }

    Ok(checked)
}

fn generated_schemas() -> Result<Vec<GeneratedSchema>, String> {
    Ok(vec![
        generated::<packs::Pack>(
            "pack",
            packs::PACK_SCHEMA_VERSION,
            "packs/schema.json",
            packs::PACK_SCHEMA_MIGRATION,
            "packs/_example.yaml",
            packs::PACK_SCHEMA_VERSION,
        )?,
        generated::<profile::Profile>(
            "profile",
            profile::PROFILE_SCHEMA_VERSION,
            "profiles/schema.json",
            profile::PROFILE_SCHEMA_MIGRATION,
            "src-tauri/fixtures/profiles/v1-valid.yaml",
            profile::LEGACY_PROFILE_SCHEMA_VERSION,
        )?,
        generated::<quirks::QuirkDocument>(
            "quirk",
            quirks::QUIRK_SCHEMA_VERSION,
            "quirks/schema.json",
            quirks::QUIRK_SCHEMA_MIGRATION,
            "quirks/hyperos.yaml",
            quirks::QUIRK_SCHEMA_VERSION,
        )?,
    ])
}

fn generated<T: JsonSchema>(
    kind: &'static str,
    document_version: &'static str,
    relative_path: &'static str,
    migration: &'static str,
    compatibility_fixture: &'static str,
    fixture_version: &'static str,
) -> Result<GeneratedSchema, String> {
    let mut value = serde_json::to_value(schema_for!(T))
        .map_err(|error| format!("could not serialize {kind} schema: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("generated {kind} schema is not an object"))?;
    object.insert(
        "$id".to_string(),
        Value::String(format!(
            "https://github.com/SysAdminDoc/Droidsmith/blob/master/{relative_path}"
        )),
    );
    object.insert(
        "x-droidsmith-document-version".to_string(),
        Value::String(document_version.to_string()),
    );
    let mut contents = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("could not format {kind} schema: {error}"))?;
    contents.push('\n');
    Ok(GeneratedSchema {
        kind,
        document_version,
        relative_path,
        migration,
        compatibility_fixture,
        fixture_version,
        contents,
    })
}

fn validate_fixture(repo_root: &Path, document: &GeneratedSchema) -> Result<(), String> {
    let path = repo_root.join(document.compatibility_fixture);
    match document.kind {
        "pack" => {
            let pack = packs::load(&path).map_err(|error| error.to_string())?;
            if pack.version != document.fixture_version {
                return Err(format!(
                    "{} has pack version {:?}, expected {:?}",
                    path.display(),
                    pack.version,
                    document.fixture_version
                ));
            }
        }
        "profile" => match profile::inspect(&path).map_err(|error| error.to_string())? {
            profile::ProfileDocument::MigrationAvailable { migration }
                if migration.from_version == document.fixture_version
                    && migration.to_version == document.document_version => {}
            _ => {
                return Err(format!(
                    "{} no longer proves migration from profile v{} to v{}",
                    path.display(),
                    document.fixture_version,
                    document.document_version
                ));
            }
        },
        "quirk" => {
            quirks::load_file(&path).map_err(|error| error.to_string())?;
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            let value: Value = serde_yaml_ng::from_str(&text)
                .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
            if value.get("version").and_then(Value::as_str) != Some(document.fixture_version) {
                return Err(format!(
                    "{} does not declare quirk version {:?}",
                    path.display(),
                    document.fixture_version
                ));
            }
        }
        other => return Err(format!("unsupported generated schema kind {other:?}")),
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_strict() {
        let first = generated_schemas().expect("schemas should generate");
        let second = generated_schemas().expect("schemas should generate twice");
        assert_eq!(first.len(), second.len());
        for (left, right) in first.iter().zip(second.iter()) {
            assert_eq!(left.contents, right.contents);
            let schema: Value = serde_json::from_str(&left.contents).expect("valid JSON schema");
            assert_eq!(
                schema.get("x-droidsmith-document-version"),
                Some(&Value::String(left.document_version.to_string()))
            );
            assert_eq!(
                schema.get("additionalProperties"),
                Some(&Value::Bool(false))
            );
        }
    }
}
