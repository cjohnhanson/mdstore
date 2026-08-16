//! The single-binary rule: this crate spawns no external program where a
//! library exists. No git, curl, aws, gcloud, sh, or kill. The only
//! spawns permitted are of a command the user declared in config, and
//! those name no program literal in source.

#[test]
fn no_source_file_spawns_a_named_program() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let banned = [
        "Command::new(\"git\"",
        "Command::new(\"curl\"",
        "Command::new(\"aws\"",
        "Command::new(\"gcloud\"",
        "Command::new(\"gsutil\"",
        "Command::new(\"sh\"",
        "Command::new(\"bash\"",
        "Command::new(\"/bin/kill\"",
        "Command::new(\"kill\"",
        "Command::new(\"ssh\"",
        "Command::new(\"gh\"",
    ];
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src/") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("read file");
                for (n, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    for b in banned {
                        if line.contains(b) {
                            offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                        }
                    }
                }
            }
        }
    }
    assert!(offenders.is_empty(), "spawns of a named program:\n{}", offenders.join("\n"));
}
