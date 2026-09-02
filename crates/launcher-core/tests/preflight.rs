//! Preflight, settings cache, and the Stage 1 first-run pass.

use launcher_core::firstrun::{
    extract_hints, FirstRun, FirstRunEvent, FirstRunState, FirstRunTarget, HintKind,
};
use launcher_core::herdr::HerdrCli;
use launcher_core::plan::{plan, LaunchRequest, PaneRef};
use launcher_core::preflight::{BinaryResolver, HerdrStatus, Issue, Preflight};
use launcher_core::registry::Registry;
use launcher_core::settings::Settings;
use launcher_core::template::Templates;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");

fn project() -> &'static Path {
    Path::new("D:\\work\\herdup")
}

/// Stubbed PATH, so tests do not depend on what is installed on the machine.
struct StubResolver(BTreeMap<String, PathBuf>);

impl StubResolver {
    fn with(found: &[&str]) -> Self {
        StubResolver(
            found
                .iter()
                .map(|b| (b.to_string(), PathBuf::from(format!("/usr/bin/{b}"))))
                .collect(),
        )
    }
}

impl BinaryResolver for StubResolver {
    fn resolve(&self, base_name: &str) -> Option<PathBuf> {
        self.0.get(base_name).cloned()
    }
}

fn client(test_name: &str, rules: Value) -> HerdrCli {
    let dir = std::env::temp_dir().join(format!("herdup-pf-{}-{}", std::process::id(), test_name));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = dir.join("script.json");
    let _ = std::fs::remove_file(dir.join("script.json.state.json"));
    std::fs::write(&script, json!({ "rules": rules }).to_string()).expect("write");
    HerdrCli::new(FAKE).with_env("FAKE_HERDR_SCRIPT", script.to_string_lossy().into_owned())
}

fn ok(body: Value) -> Value {
    json!({ "stdout": json!({ "id": "t", "result": body }).to_string(), "exit": 0 })
}

fn api_error(code: &str, message: &str) -> Value {
    json!({
        "stderr": json!({ "id": "t", "error": { "code": code, "message": message } }).to_string(),
        "exit": 1
    })
}

fn pane(id: &str, status: &str) -> Value {
    json!({ "pane_id": id, "tab_id": "w1:t1", "workspace_id": "w1", "agent_status": status })
}

fn healthy_herdr() -> Vec<Value> {
    vec![
        json!({ "match": ["--version"], "responses": [{ "stdout": "herdr 0.8.2-preview.x", "exit": 0 }] }),
        json!({ "match": ["workspace", "list"], "responses": [ok(json!({ "workspaces": [] }))] }),
    ]
}

fn squad_plan(swap: Option<(usize, &str)>) -> launcher_core::plan::LaunchPlan {
    let reg = Registry::builtin();
    let templates = Templates::builtin();
    let t = templates.get("squad").expect("squad");
    let mut req = LaunchRequest::new(project(), t);
    if let Some((i, cli)) = swap {
        req = req.override_cli(i, cli);
    }
    plan(&req, &reg).expect("plans")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-set-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let _ = std::fs::remove_file(dir.join("settings.toml"));
    dir
}

// ---------------------------------------------------------------------------
// preflight
// ---------------------------------------------------------------------------

#[test]
fn a_healthy_environment_reports_no_issues() {
    let cli = client("healthy", Value::Array(healthy_herdr()));
    let pf = Preflight::run(
        &cli,
        &squad_plan(None),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );

    assert!(matches!(pf.herdr, HerdrStatus::Ready { .. }));
    assert!(pf.can_launch(), "issues: {:?}", pf.issues());
    assert_eq!(pf.clis.len(), 1, "four claude panes collapse to one check");
    assert!(pf.clis[0].installed());
    assert_eq!(pf.clis[0].panes.len(), 4);
}

#[test]
fn a_missing_cli_blocks_the_launch_and_names_the_panes_that_need_it() {
    let cli = client("missing", Value::Array(healthy_herdr()));
    let pf = Preflight::run(
        &cli,
        &squad_plan(Some((3, "codex"))),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]), // codex absent
    );

    assert!(!pf.can_launch());
    let issues = pf.issues();
    assert_eq!(issues.len(), 1);
    match &issues[0] {
        Issue::CliMissing {
            cli,
            panes,
            docs_url,
            ..
        } => {
            assert_eq!(cli, "codex");
            assert_eq!(panes, &vec![PaneRef(3)], "names the pane that needs it");
            assert!(docs_url.is_some(), "offers somewhere to go");
        }
        other => panic!("expected CliMissing, got {other:?}"),
    }
}

#[test]
fn a_missing_cli_offers_installed_alternatives_to_switch_to() {
    let cli = client("alts", Value::Array(healthy_herdr()));
    let pf = Preflight::run(
        &cli,
        &squad_plan(Some((3, "codex"))),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );

    let alts = pf.alternatives_for("codex");
    assert_eq!(alts.len(), 1);
    assert_eq!(alts[0].id, "claude", "only offer CLIs actually installed");
}

#[test]
fn claude_carries_a_platform_install_command() {
    let cli = client("hint", Value::Array(healthy_herdr()));
    let pf = Preflight::run(
        &cli,
        &squad_plan(None),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&[]),
    );
    let hint = pf.clis[0].install_command.as_deref().expect("has a hint");
    if cfg!(windows) {
        assert!(hint.contains("install.ps1"), "{hint}");
    } else {
        assert!(hint.contains("install.sh"), "{hint}");
    }
}

#[test]
fn a_stopped_server_is_reported_as_recoverable() {
    let cli = client(
        "serverdown",
        json!([
            { "match": ["--version"], "responses": [{ "stdout": "herdr 0.8.2", "exit": 0 }] },
            { "match": ["workspace", "list"], "responses": [api_error("server_not_running", "none")] },
        ]),
    );
    let pf = Preflight::run(
        &cli,
        &squad_plan(None),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );
    assert!(matches!(pf.herdr, HerdrStatus::ServerDown { .. }));
    assert!(pf.issues().iter().any(Issue::is_server_startable));

    // herdup starts a server for its own session at launch, so this must not be
    // presented as something the user has to go and fix.
    assert!(
        pf.can_launch(),
        "a stopped server is auto-resolvable, not blocking"
    );
    assert!(pf.blocking_issues().is_empty());
    assert_eq!(pf.auto_resolvable_issues().len(), 1);
}

#[test]
fn a_missing_cli_is_blocking_while_a_stopped_server_is_not() {
    let cli = client(
        "mixed",
        json!([
            { "match": ["--version"], "responses": [{ "stdout": "herdr 0.8.2", "exit": 0 }] },
            { "match": ["workspace", "list"], "responses": [api_error("server_not_running", "none")] },
        ]),
    );
    let pf = Preflight::run(
        &cli,
        &squad_plan(Some((3, "codex"))),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );

    assert_eq!(pf.issues().len(), 2);
    assert_eq!(pf.blocking_issues().len(), 1, "only the missing CLI blocks");
    assert!(matches!(pf.blocking_issues()[0], Issue::CliMissing { .. }));
    assert!(!pf.can_launch());
}

#[test]
fn a_protocol_mismatch_is_reported_as_not_ours_to_fix() {
    // Resolving it means stopping the user's server, which kills their panes.
    let cli = client(
        "proto",
        json!([
            { "match": ["--version"], "responses": [{ "stdout": "herdr 0.8.2", "exit": 0 }] },
            { "match": ["workspace", "list"], "responses": [api_error("protocol_mismatch", "client 21 server 20")] },
        ]),
    );
    let pf = Preflight::run(
        &cli,
        &squad_plan(None),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );
    assert!(matches!(pf.herdr, HerdrStatus::ProtocolMismatch { .. }));
    let issues = pf.issues();
    assert!(matches!(issues[0], Issue::HerdrProtocolMismatch { .. }));
    assert!(
        !issues[0].is_server_startable(),
        "must not offer to restart it"
    );
}

#[test]
fn an_old_herdr_is_rejected_because_pane_ids_compact_below_0_8() {
    let cli = client(
        "old",
        json!([{ "match": ["--version"], "responses": [{ "stdout": "herdr 0.7.0", "exit": 0 }] }]),
    );
    let pf = Preflight::run(
        &cli,
        &squad_plan(None),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );
    match &pf.herdr {
        HerdrStatus::TooOld { found, required } => {
            assert!(found.starts_with("0.7"));
            assert_eq!(required, "0.8.2");
        }
        other => panic!("expected TooOld, got {other:?}"),
    }
    assert!(!pf.can_launch());
}

// ---------------------------------------------------------------------------
// the verification cache
// ---------------------------------------------------------------------------

#[test]
fn a_cache_hit_skips_first_run_and_a_miss_does_not() {
    let cli = client("cache", Value::Array(healthy_herdr()));
    let reg = Registry::builtin();
    let plan = squad_plan(None);
    let resolver = StubResolver::with(&["claude"]);

    let cold = Preflight::run(&cli, &plan, &reg, &Settings::default(), &resolver);
    assert_eq!(cold.needs_first_run().len(), 1);

    let mut settings = Settings::default();
    settings.mark_verified("claude", project());
    let warm = Preflight::run(&cli, &plan, &reg, &settings, &resolver);
    assert!(warm.needs_first_run().is_empty());
}

#[test]
fn the_cache_is_scoped_to_the_project_not_just_the_cli() {
    // A trust prompt is per-folder, so a CLI trusted in one repo is not trusted
    // in the next. Using the tighter key means we never wrongly skip.
    let mut settings = Settings::default();
    settings.mark_verified("claude", Path::new("D:\\work\\a"));
    assert!(settings.is_verified("claude", Path::new("D:\\work\\a")));
    assert!(!settings.is_verified("claude", Path::new("D:\\work\\b")));
}

#[test]
fn the_cache_key_ignores_a_trailing_separator() {
    // herdr reports the same directory both ways depending on pane state.
    let mut settings = Settings::default();
    settings.mark_verified("claude", Path::new("D:\\work\\a\\"));
    assert!(settings.is_verified("claude", Path::new("D:\\work\\a")));
}

#[test]
fn a_missing_cli_is_not_offered_for_first_run() {
    let cli = client("missing_fr", Value::Array(healthy_herdr()));
    let pf = Preflight::run(
        &cli,
        &squad_plan(Some((3, "codex"))),
        &Registry::builtin(),
        &Settings::default(),
        &StubResolver::with(&["claude"]),
    );
    let names: Vec<&str> = pf.needs_first_run().iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        names,
        vec!["claude"],
        "codex is missing, not merely unverified"
    );
}

#[test]
fn settings_round_trip_through_disk() {
    let dir = temp_dir("roundtrip");
    let mut settings = Settings {
        projects_root: Some("D:\\work".into()),
        ..Default::default()
    };
    settings.mark_verified("claude", project());
    settings.save_to(&dir).expect("saves");

    let loaded = Settings::load_from(Some(&dir));
    assert_eq!(loaded.projects_root.as_deref(), Some("D:\\work"));
    assert!(loaded.is_verified("claude", project()));
    assert_eq!(loaded.verified.len(), 1);
}

#[test]
fn a_corrupt_settings_file_degrades_to_defaults_rather_than_failing() {
    // Worst case is that Stage 1 runs again, which is safe. Refusing to launch
    // because of a bad settings file would not be.
    let dir = temp_dir("corrupt");
    std::fs::write(dir.join("settings.toml"), "this is not toml {{{").unwrap();
    let loaded = Settings::load_from(Some(&dir));
    assert_eq!(loaded, Settings::default());
}

#[test]
fn marking_verified_twice_does_not_duplicate_the_record() {
    let mut settings = Settings::default();
    settings.mark_verified("claude", project());
    settings.mark_verified("claude", project());
    assert_eq!(settings.verified.len(), 1);

    settings.forget("claude", project());
    assert!(settings.verified.is_empty());
}

#[test]
fn unverified_filters_a_list_of_clis() {
    let mut settings = Settings::default();
    settings.mark_verified("claude", project());
    assert_eq!(
        settings.unverified(&["claude", "codex", "droid"], project()),
        vec!["codex", "droid"]
    );
}

// ---------------------------------------------------------------------------
// hint extraction
// ---------------------------------------------------------------------------

#[test]
fn a_device_flow_screen_yields_a_url_and_a_code() {
    let screen = "\
        ! First copy your one-time code: WDJB-MJHT\n\
        Press Enter to open github.com in your browser...\n\
        Open https://github.com/login/device and paste it.\n";
    let hints = extract_hints(screen);

    let urls: Vec<&str> = hints
        .iter()
        .filter(|h| h.kind == HintKind::Url)
        .map(|h| h.value.as_str())
        .collect();
    assert_eq!(urls, vec!["https://github.com/login/device"]);

    let codes: Vec<&str> = hints
        .iter()
        .filter(|h| h.kind == HintKind::DeviceCode)
        .map(|h| h.value.as_str())
        .collect();
    assert_eq!(codes, vec!["WDJB-MJHT"]);
}

#[test]
fn urls_are_cleaned_of_surrounding_punctuation() {
    let hints = extract_hints("visit (https://auth.example.com/activate), then return.");
    assert_eq!(hints[0].value, "https://auth.example.com/activate");
}

#[test]
fn ordinary_hyphenated_words_are_not_mistaken_for_device_codes() {
    // A loose pattern would put a copy button on half the screen.
    let hints = extract_hints("first-run trust-this-folder Claude-Code well-known 12-34 ab-cd");
    assert!(
        hints.is_empty(),
        "false positives: {:?}",
        hints.iter().map(|h| &h.value).collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_hints_are_reported_once() {
    let hints = extract_hints("https://a.example https://a.example ABCD-1234 ABCD-1234");
    assert_eq!(hints.len(), 2);
}

// ---------------------------------------------------------------------------
// Stage 1 orchestration
// ---------------------------------------------------------------------------

fn firstrun_rules(extra: Vec<Value>) -> Value {
    let mut all = extra;
    all.extend(vec![
        json!({
            "match": ["workspace", "create"],
            "responses": [ok(json!({
                "workspace": { "workspace_id": "w9", "label": "herdup first-run" },
                "tab": { "tab_id": "w9:t1", "workspace_id": "w9" },
                "root_pane": pane("w9:p1", "unknown")
            }))]
        }),
        json!({ "match": ["pane", "split"], "responses": [ok(json!({ "pane": pane("w9:p2", "unknown") }))] }),
        json!({ "match": ["pane", "rename"], "responses": [ok(json!({ "pane": pane("w9:p1", "unknown") }))] }),
        json!({ "match": ["pane", "run"], "responses": [{ "exit": 0 }] }),
        json!({ "match": ["pane", "close"], "responses": [{ "exit": 0 }] }),
        json!({ "match": ["workspace", "close"], "responses": [{ "exit": 0 }] }),
    ]);
    Value::Array(all)
}

fn targets() -> Vec<FirstRunTarget> {
    vec![FirstRunTarget {
        cli: "claude".into(),
        display_name: "Claude Code".into(),
        binary: "claude".into(),
    }]
}

#[test]
fn a_first_run_pane_runs_the_bare_binary_with_no_flags() {
    // Permission flags are irrelevant to signing in, and could suppress the
    // very prompt we are trying to surface. The rule matches the exact argv.
    let cli = client(
        "bare",
        firstrun_rules(vec![
            json!({ "match": ["pane", "run", "w9:p1", "claude"], "responses": [{ "exit": 0 }] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w9:p1", "unknown") }))] }),
            json!({ "match": ["pane", "read"], "responses": [{ "stdout": "starting", "exit": 0 }] }),
        ]),
    );
    let session = FirstRun::new(&cli)
        .start(project(), &targets(), &mut |_| {})
        .expect("starts");
    assert_eq!(session.workspace_id, "w9");
    assert_eq!(session.panes[0].pane_id, "w9:p1");
    assert_eq!(session.panes[0].state, FirstRunState::Waiting);
}

#[test]
fn a_cli_reaching_its_prompt_is_marked_verified_and_cached() {
    let cli = client(
        "verify",
        firstrun_rules(vec![
            json!({
                "match": ["pane", "get"],
                "responses": [
                    ok(json!({ "pane": pane("w9:p1", "blocked") })),
                    ok(json!({ "pane": pane("w9:p1", "idle") }))
                ]
            }),
            json!({
                "match": ["pane", "read"],
                "responses": [
                    { "stdout": "Trust this folder? Visit https://x.example ABCD-EFGH", "exit": 0 },
                    { "stdout": "> ready", "exit": 0 }
                ]
            }),
        ]),
    );
    let fr = FirstRun::new(&cli);
    let mut events = Vec::new();
    let mut session = fr
        .start(project(), &targets(), &mut |e| events.push(e))
        .expect("starts");

    fr.poll_once(&mut session, &mut |e| events.push(e));
    assert_eq!(session.panes[0].state, FirstRunState::NeedsYou);
    assert!(!session.all_verified());
    // Hints from the blocking screen are surfaced for copying.
    assert_eq!(session.panes[0].hints.len(), 2);
    assert!(events
        .iter()
        .any(|e| matches!(e, FirstRunEvent::HintFound { .. })));

    // The human deals with it; the next poll sees the prompt.
    fr.poll_once(&mut session, &mut |e| events.push(e));
    assert_eq!(session.panes[0].state, FirstRunState::Verified);
    assert!(session.all_verified());

    let mut settings = Settings::default();
    session.apply_to(&mut settings);
    assert!(settings.is_verified("claude", project()));
}

#[test]
fn an_abandoned_pass_caches_nothing() {
    // Only CLIs that actually reached their prompt are recorded, so cancelling
    // leaves the next launch asking again rather than wrongly skipping.
    let cli = client(
        "abandon",
        firstrun_rules(vec![
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w9:p1", "blocked") }))] }),
            json!({ "match": ["pane", "read"], "responses": [{ "stdout": "waiting", "exit": 0 }] }),
        ]),
    );
    let fr = FirstRun::new(&cli);
    let mut session = fr
        .start(project(), &targets(), &mut |_| {})
        .expect("starts");
    fr.poll_once(&mut session, &mut |_| {});

    let mut settings = Settings::default();
    session.apply_to(&mut settings);
    assert!(settings.verified.is_empty());

    // And teardown removes the throwaway workspace.
    fr.teardown(&session).expect("tears down");
}

#[test]
fn teardown_closes_the_workspace_even_if_a_pane_refuses_to_close() {
    // One stubborn pane must not leave the setup workspace behind.
    let cli = client(
        "teardown",
        firstrun_rules(vec![
            json!({ "match": ["pane", "close"], "responses": [api_error("pane_busy", "nope")] }),
            json!({ "match": ["pane", "get"], "responses": [ok(json!({ "pane": pane("w9:p1", "idle") }))] }),
            json!({ "match": ["pane", "read"], "responses": [{ "stdout": "> ", "exit": 0 }] }),
        ]),
    );
    let fr = FirstRun::new(&cli);
    let session = fr
        .start(project(), &targets(), &mut |_| {})
        .expect("starts");
    fr.teardown(&session).expect("workspace still closed");
}
