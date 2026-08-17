//! The merge gate's pull-request guard.
//!
//! `scripts/merge-gate.sh` reads the review note from the pull request
//! head instead of the checked-out merge commit. Two things must hold.
//! A pull request whose head carries no note is refused. A local shell
//! that sets `GITHUB_EVENT_NAME` alone does not turn the note check
//! off, because the guard also requires the runner.
//!
//! These tests drive the script directly. They synthesize the ref line
//! git sends on stdin, so nothing pushes and no network runs.

use std::io::Write as _;
use std::process::{Command, Stdio};

/// Runs the note section of the gate against one sha, with the given
/// environment. Returns the exit code and the merged output.
///
/// `SKIP_TESTS` short-circuits the cargo and missouri sections, so the
/// assertion covers the note branch rather than a full test run.
fn run_gate(sha: &str, env: &[(&str, &str)], event_path: Option<&str>) -> (i32, String) {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut cmd = Command::new("sh");
    cmd.arg("scripts/merge-gate.sh")
        .current_dir(root)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_EVENT_PATH")
        .env("MERGE_GATE_SKIP_TESTS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(p) = event_path {
        cmd.env("GITHUB_EVENT_PATH", p);
    }

    let mut child = cmd.spawn().expect("merge-gate.sh runs");
    let line = format!(
        "refs/heads/topic {sha} refs/heads/main 0000000000000000000000000000000000000000\n"
    );
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(line.as_bytes())
        .expect("the ref line writes");
    let out = child.wait_with_output().expect("the gate finishes");
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

/// A sha with no review note. The empty tree object always exists and
/// never carries one.
const NOTELESS: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[test]
fn a_local_shell_cannot_skip_the_note_check() {
    // Only the event name, no runner. The guard must not fire.
    let (code, out) = run_gate(NOTELESS, &[("GITHUB_EVENT_NAME", "pull_request")], None);
    assert_ne!(code, 0, "a local shell skipped the note check: {out}");
    assert!(
        out.contains("no fresh-eyes review note"),
        "expected the note refusal, got: {out}"
    );
}

#[test]
fn a_push_event_checks_the_note() {
    let (code, out) = run_gate(
        NOTELESS,
        &[("GITHUB_ACTIONS", "true"), ("GITHUB_EVENT_NAME", "push")],
        None,
    );
    assert_ne!(code, 0, "a push event skipped the note check: {out}");
}

#[test]
fn an_unset_event_checks_the_note() {
    let (code, out) = run_gate(NOTELESS, &[("GITHUB_ACTIONS", "true")], None);
    assert_ne!(code, 0, "an unset event skipped the note check: {out}");
}

#[test]
fn a_pull_request_refuses_a_head_with_no_note() {
    let dir = std::env::temp_dir().join("mdstore-gate-guard-noteless");
    std::fs::create_dir_all(&dir).expect("the temp directory is made");
    let event = dir.join("event.json");
    std::fs::write(
        &event,
        format!(r#"{{"pull_request":{{"head":{{"sha":"{NOTELESS}"}}}}}}"#),
    )
    .expect("the event payload writes");

    let (code, out) = run_gate(
        NOTELESS,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
        ],
        event.to_str(),
    );
    assert_ne!(code, 0, "a note-less pull request head passed: {out}");
    assert!(
        out.contains("no fresh-eyes review note"),
        "expected the note refusal, got: {out}"
    );
}

#[test]
fn a_pull_request_refuses_an_unreadable_head_sha() {
    let dir = std::env::temp_dir().join("mdstore-gate-guard-badpayload");
    std::fs::create_dir_all(&dir).expect("the temp directory is made");
    let event = dir.join("event.json");
    std::fs::write(&event, r#"{"pull_request":{}}"#).expect("the event payload writes");

    let (code, out) = run_gate(
        NOTELESS,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
        ],
        event.to_str(),
    );
    assert_ne!(code, 0, "an unreadable head sha passed: {out}");
    assert!(
        out.contains("unreadable"),
        "expected the unreadable-sha refusal, got: {out}"
    );
}
