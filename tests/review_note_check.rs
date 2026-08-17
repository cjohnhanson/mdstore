//! `scripts/review-note-check.sh`.
//!
//! The check reads git's pre-push ref lines on stdin and refuses a tip
//! that carries no fresh-eyes review note. On a pull request event it
//! reads the branch head instead of the merge commit GitHub creates,
//! which no reviewer ever saw.
//!
//! The script runs no build tool, so these tests call it directly.

use std::io::Write as _;
use std::process::{Command, Stdio};

/// Feeds one ref line to the check and returns its exit code and output.
fn check(sha: &str, env: &[(&str, &str)], event_path: Option<&str>) -> (i32, String) {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut cmd = Command::new("sh");
    cmd.arg("scripts/review-note-check.sh")
        .current_dir(root)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_EVENT_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(p) = event_path {
        cmd.env("GITHUB_EVENT_PATH", p);
    }

    let mut child = cmd.spawn().expect("the check runs");
    let line = format!(
        "refs/heads/topic {sha} refs/heads/main 0000000000000000000000000000000000000000\n"
    );
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(line.as_bytes())
        .expect("the ref line writes");
    let out = child.wait_with_output().expect("the check finishes");
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

/// Writes a pull request event payload naming one head sha.
fn event_payload(name: &str, sha: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).expect("the temp directory is made");
    let p = dir.join("event.json");
    std::fs::write(
        &p,
        format!(r#"{{"pull_request":{{"head":{{"sha":"{sha}"}}}}}}"#),
    )
    .expect("the payload writes");
    p
}

/// The empty tree object. It exists in every repository and never
/// carries a review note.
const NOTELESS: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// HEAD, which carries a note whenever a push has been prepared.
fn head() -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git rev-parse runs");
    String::from_utf8(out.stdout)
        .expect("the sha is utf-8")
        .trim()
        .to_string()
}

fn head_has_note() -> bool {
    Command::new("git")
        .args(["notes", "--ref=reviews", "show", &head()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_tip_with_no_note_is_refused() {
    let (code, out) = check(NOTELESS, &[], None);
    assert_ne!(code, 0, "a note-less tip passed: {out}");
    assert!(out.contains("no fresh-eyes review note"), "got: {out}");
}

#[test]
fn a_local_shell_cannot_claim_to_be_a_pull_request() {
    // The event name alone must not switch the check to head-reading.
    // The runner sets both variables; a local shell sets neither.
    let (code, out) = check(NOTELESS, &[("GITHUB_EVENT_NAME", "pull_request")], None);
    assert_ne!(code, 0, "a local shell skipped the check: {out}");
    assert!(out.contains("no fresh-eyes review note"), "got: {out}");
}

#[test]
fn a_push_event_checks_the_tip() {
    let (code, out) = check(
        NOTELESS,
        &[("GITHUB_ACTIONS", "true"), ("GITHUB_EVENT_NAME", "push")],
        None,
    );
    assert_ne!(code, 0, "a push event skipped the check: {out}");
}

#[test]
fn a_pull_request_refuses_a_head_with_no_note() {
    let ev = event_payload("mdstore-note-noteless", NOTELESS);
    let (code, out) = check(
        NOTELESS,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
        ],
        ev.to_str(),
    );
    assert_ne!(code, 0, "a note-less pull request head passed: {out}");
    assert!(out.contains("no fresh-eyes review note"), "got: {out}");
}

#[test]
fn a_pull_request_refuses_an_unreadable_head_sha() {
    let dir = std::env::temp_dir().join("mdstore-note-badpayload");
    std::fs::create_dir_all(&dir).expect("the temp directory is made");
    let ev = dir.join("event.json");
    std::fs::write(&ev, r#"{"pull_request":{}}"#).expect("the payload writes");

    let (code, out) = check(
        NOTELESS,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
        ],
        ev.to_str(),
    );
    assert_ne!(code, 0, "an unreadable head sha passed: {out}");
    assert!(out.contains("unreadable"), "got: {out}");
}

#[test]
fn a_pull_request_accepts_a_head_that_carries_a_note() {
    // Every other test asserts a refusal, so a check broken open in the
    // wrong direction would pass them all. This one pins the accepting
    // path.
    if !head_has_note() {
        eprintln!("skipped: HEAD carries no review note");
        return;
    }
    let h = head();
    let ev = event_payload("mdstore-note-noted", &h);
    let (code, out) = check(
        &h,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
        ],
        ev.to_str(),
    );
    assert_eq!(code, 0, "a reviewed head was refused: {out}");
    assert!(out.contains("Reading the note on"), "got: {out}");
}

#[test]
fn a_notes_ref_push_is_exempt() {
    // Pushing the reviews ref shares review records. It merges nothing,
    // so it carries no note of its own.
    let root = env!("CARGO_MANIFEST_DIR");
    let mut child = Command::new("sh")
        .arg("scripts/review-note-check.sh")
        .current_dir(root)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_EVENT_NAME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the check runs");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(
            format!("refs/notes/reviews {NOTELESS} refs/notes/reviews 0000000000000000000000000000000000000000\n")
                .as_bytes(),
        )
        .expect("the ref line writes");
    let out = child.wait_with_output().expect("the check finishes");
    assert_eq!(
        out.status.code(),
        Some(0),
        "a notes-ref push was refused: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
