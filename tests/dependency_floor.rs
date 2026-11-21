//! Dependency floors that keep a consumer off a known vulnerability.
//!
//! A consumer resolves this crate's requirements against its own lock,
//! so the floor in Cargo.toml, not this repository's lock, decides
//! whether the consumer can stay on a vulnerable version. Each entry
//! names the advisory that set it.

/// (dependency, lowest safe version, advisory).
const FLOORS: &[(&str, (u64, u64, u64), &str)] = &[
    // A path ending in "/" followed a symlink out of the confined store.
    ("cap-std", (4, 0, 3), "RUSTSEC-2026-0482"),
];

/// The version requirement Cargo.toml gives a dependency, from a line
/// of the form `name = "x.y.z"`.
fn requirement(manifest: &str, name: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(name)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim();
        Some(rest.trim_matches('"').trim_start_matches('^').to_string())
    })
}

fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

#[test]
fn every_floor_excludes_its_advisory() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = std::fs::read_to_string(path).expect("read Cargo.toml");
    for (name, safe, advisory) in FLOORS {
        let found = requirement(&manifest, name)
            .unwrap_or_else(|| panic!("{name} has no `{name} = \"x.y.z\"` line in Cargo.toml"));
        let floor = parse(&found)
            .unwrap_or_else(|| panic!("{name} = {found:?} is not a full x.y.z version"));
        assert!(
            floor >= *safe,
            "{name} = {found:?} admits a version {advisory} affects; the floor must be at least {}.{}.{}",
            safe.0,
            safe.1,
            safe.2
        );
    }
}

#[test]
fn the_requirement_reader_finds_the_version() {
    let manifest = "rand = \"0.10.0\"\n# cap-std note\ncap-std = \"4.0.2\"\n";
    assert_eq!(requirement(manifest, "cap-std").as_deref(), Some("4.0.2"));
    assert_eq!(parse("4.0.2"), Some((4, 0, 2)));
    assert!(parse("4.0.2").unwrap() < (4, 0, 3));
    assert_eq!(parse("4.0"), None);
}
