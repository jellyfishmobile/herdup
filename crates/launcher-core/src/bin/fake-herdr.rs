//! A scriptable stand-in for the `herdr` binary.
//!
//! Lets the integration tests drive `HerdrCli` through the real spawn path with
//! no herdr installed, and — crucially — script `agent_status` transitions so
//! sequences like "blocked, then idle after a sign-in" are CI-testable.
//!
//! Point `FAKE_HERDR_SCRIPT` at a JSON file:
//!
//! ```json
//! {
//!   "rules": [
//!     { "match": ["pane", "get", "w1:p1"],
//!       "responses": [
//!         { "stdout": "{\"result\":{...blocked...}}", "exit": 0 },
//!         { "stdout": "{\"result\":{...idle...}}",    "exit": 0 }
//!       ] }
//!   ]
//! }
//! ```
//!
//! `match` is matched as an ordered subsequence of argv, so a rule need only
//! name the distinguishing tokens. The first matching rule wins. Its
//! `responses` are consumed one per call, and the last one repeats forever.
//! Call counts persist in `<script>.state.json`.
//!
//! An unmatched invocation exits 97 so a test fails loudly rather than
//! silently taking an unintended path.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Script {
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct Rule {
    #[serde(rename = "match")]
    pattern: Vec<String>,
    responses: Vec<Response>,
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    exit: i32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// rule index -> how many times it has fired
    counts: HashMap<String, usize>,
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let script_path = match std::env::var("FAKE_HERDR_SCRIPT") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            eprintln!("fake-herdr: FAKE_HERDR_SCRIPT is not set");
            std::process::exit(98);
        }
    };

    let script: Script = match std::fs::read_to_string(&script_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(s) => s,
        None => {
            eprintln!(
                "fake-herdr: could not read script {}",
                script_path.display()
            );
            std::process::exit(98);
        }
    };

    let Some((index, rule)) = script
        .rules
        .iter()
        .enumerate()
        .find(|(_, r)| is_subsequence(&r.pattern, &argv))
    else {
        eprintln!("fake-herdr: no rule matched argv {argv:?}");
        std::process::exit(97);
    };

    let state_path = state_path(&script_path);
    let mut state: State = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();

    let key = index.to_string();
    let seen = state.counts.entry(key).or_insert(0);
    // Last response repeats, so a rule can describe a settled steady state.
    let response = &rule.responses[(*seen).min(rule.responses.len().saturating_sub(1))];
    *seen += 1;

    if let Ok(text) = serde_json::to_string(&state) {
        let _ = std::fs::write(&state_path, text);
    }

    if !response.stdout.is_empty() {
        print!("{}", response.stdout);
        let _ = std::io::stdout().flush();
    }
    if !response.stderr.is_empty() {
        eprint!("{}", response.stderr);
        let _ = std::io::stderr().flush();
    }
    std::process::exit(response.exit);
}

fn state_path(script: &Path) -> PathBuf {
    let mut p = script.to_path_buf();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "script".into());
    p.set_file_name(format!("{name}.state.json"));
    p
}

/// True when every token of `pattern` appears in `argv`, in order.
fn is_subsequence(pattern: &[String], argv: &[String]) -> bool {
    let mut it = argv.iter();
    pattern.iter().all(|want| it.any(|got| got == want))
}
