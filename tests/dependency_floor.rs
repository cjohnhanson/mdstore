//! Dependency floors that keep a consumer off a known vulnerability.
//!
//! A consumer resolves this crate's requirements against its own lock,
//! so the floor in Cargo.toml, not this repository's lock, decides
//! whether the consumer can stay on a vulnerable version. Each entry
//! names the advisory that set it.

use toml::{Table, Value};

/// (dependency, lowest safe version, advisory).
const FLOORS: &[(&str, (u64, u64, u64), &str)] = &[
    // A path ending in "/" followed a symlink out of the confined store.
    ("cap-std", (4, 0, 3), "RUSTSEC-2026-0482"),
];

const KINDS: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

/// Every version requirement the manifest gives a dependency: in each
/// dependency table, and in each target's tables, in string or table
/// form.
fn requirements(manifest: &Table, name: &str) -> Vec<String> {
    let mut tables: Vec<&Table> = KINDS
        .iter()
        .filter_map(|k| manifest.get(*k).and_then(Value::as_table))
        .collect();
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            tables.extend(
                KINDS
                    .iter()
                    .filter_map(|k| target.get(*k).and_then(Value::as_table)),
            );
        }
    }
    tables
        .into_iter()
        .filter_map(|t| t.get(name))
        .map(|dep| match dep {
            Value::String(v) => v.clone(),
            Value::Table(t) => t
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// The lowest version a requirement admits, for the plain `x.y.z` and
/// caret `^x.y.z` forms. Any other form is refused, so the test fails
/// rather than guessing.
fn floor(requirement: &str) -> Option<(u64, u64, u64)> {
    let version = requirement.strip_prefix('^').unwrap_or(requirement);
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    let floor = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(floor)
}

fn check(manifest: &Table) -> Vec<String> {
    let mut failures = Vec::new();
    for (name, safe, advisory) in FLOORS {
        let found = requirements(manifest, name);
        if found.is_empty() {
            failures.push(format!("{name} is not declared in Cargo.toml"));
        }
        for requirement in found {
            match floor(&requirement) {
                Some(f) if f >= *safe => {}
                Some(_) => failures.push(format!(
                    "{name} = {requirement:?} admits a version {advisory} affects; \
                     the floor must be at least {}.{}.{}",
                    safe.0, safe.1, safe.2
                )),
                None => failures.push(format!(
                    "{name} = {requirement:?}: write the requirement as x.y.z"
                )),
            }
        }
    }
    failures
}

#[test]
fn every_floor_excludes_its_advisory() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(path).expect("read Cargo.toml");
    let manifest: Table = text.parse().expect("Cargo.toml is TOML");
    let failures = check(&manifest);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_declaration_is_checked_in_every_form() {
    let failing = |text: &str| check(&text.parse::<Table>().unwrap());
    assert!(failing("[dependencies]\ncap-std = \"4.0.3\" # note\n").is_empty());
    assert!(failing("[dependencies.cap-std]\nversion = \"^4.0.4\"\n").is_empty());
    for bad in [
        "[dependencies]\ncap-std = \"4.0.2\"\n",
        "[dependencies]\ncap-std = { version = \"4.0.2\", features = [] }\n",
        "[dependencies]\ncap-std = \"4.0.3\"\n[target.'cfg(windows)'.dependencies]\ncap-std = \"4.0.2\"\n",
        "[dependencies]\ncap-std = \">=4.0.2\"\n",
        "[dependencies]\ncap-std = \"4.0\"\n",
        "[dependencies]\nrand = \"0.10\"\n",
    ] {
        assert!(!failing(bad).is_empty(), "accepted:\n{bad}");
    }
}
