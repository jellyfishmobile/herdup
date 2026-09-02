//! GitHub repo creation: argv construction and pre-flight validation.
//!
//! Creating a repository is the one outward-facing thing herdup does, and it
//! cannot be quietly undone. So everything checkable is tested here, against no
//! network and no account.

use launcher_core::github::{validate_name, GhError, NewRepo, Visibility};
use std::path::PathBuf;

fn temp_parent(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("herdup-gh-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

// ---------------------------------------------------------------------------
// argv
// ---------------------------------------------------------------------------

#[test]
fn a_private_repo_under_the_default_account() {
    let repo = NewRepo::private("my-api", "D:\\work");
    assert_eq!(
        repo.args(),
        vec!["repo", "create", "my-api", "--private", "--clone"]
    );
}

#[test]
fn an_owner_is_prefixed_to_the_name() {
    let mut repo = NewRepo::private("my-api", "D:\\work");
    repo.owner = Some("jellyfishmobile".into());
    assert_eq!(repo.args()[2], "jellyfishmobile/my-api");
}

#[test]
fn public_must_be_asked_for_explicitly() {
    // The default is private: a repo created by accident should not be
    // world-readable.
    assert_eq!(NewRepo::private("x", ".").visibility, Visibility::Private);
    assert!(NewRepo::private("x", ".")
        .args()
        .contains(&"--private".to_string()));

    let mut repo = NewRepo::private("x", ".");
    repo.visibility = Visibility::Public;
    let args = repo.args();
    assert!(args.contains(&"--public".to_string()));
    assert!(!args.contains(&"--private".to_string()));
}

#[test]
fn a_description_becomes_its_own_argument() {
    // Its own argv element, so quotes and spaces in a description can never be
    // reinterpreted as shell syntax.
    let mut repo = NewRepo::private("my-api", "D:\\work");
    repo.description = Some(r#"A "quoted" thing; with punctuation"#.into());
    let args = repo.args();
    let i = args
        .iter()
        .position(|a| a == "--description")
        .expect("present");
    assert_eq!(args[i + 1], r#"A "quoted" thing; with punctuation"#);
    assert_eq!(args.len(), 7);
}

#[test]
fn the_echoed_command_quotes_multi_word_arguments() {
    // A live run printed `--description Throwaway` for a multi-word
    // description, which reads as if the value had been truncated. The argv is
    // fine; the echo was not.
    let mut repo = NewRepo::private("my-api", "D:\\work");
    repo.description = Some("Throwaway repo. Safe to delete.".into());
    let shown = repo.display_command();
    assert!(
        shown.contains("\"Throwaway repo. Safe to delete.\""),
        "unquoted: {shown}"
    );
    assert!(shown.starts_with("gh repo create my-api --private --clone"));
}

#[test]
fn the_echoed_command_leaves_simple_arguments_bare() {
    let repo = NewRepo::private("my-api", "D:\\work");
    assert_eq!(
        repo.display_command(),
        "gh repo create my-api --private --clone"
    );
}

#[test]
fn an_empty_description_is_omitted_entirely() {
    let mut repo = NewRepo::private("my-api", "D:\\work");
    repo.description = Some("   ".into());
    assert!(!repo.args().contains(&"--description".to_string()));
}

#[test]
fn the_clone_always_happens_so_a_launch_can_follow() {
    assert!(NewRepo::private("x", ".")
        .args()
        .contains(&"--clone".to_string()));
}

#[test]
fn the_destination_is_the_parent_plus_the_name() {
    let repo = NewRepo::private("my-api", "D:\\work");
    assert_eq!(repo.destination(), PathBuf::from("D:\\work").join("my-api"));
}

// ---------------------------------------------------------------------------
// name validation
// ---------------------------------------------------------------------------

#[test]
fn ordinary_names_are_accepted() {
    for name in ["my-api", "herdup", "a", "repo_1", "dot.name", "_leading"] {
        validate_name(name).unwrap_or_else(|e| panic!("{name} should be valid: {e}"));
    }
}

#[test]
fn names_that_github_would_reject_are_caught_before_the_network() {
    // gh would reject most of these too, but only after a round trip and with a
    // message that reads like an API error.
    let cases: Vec<(&str, &str)> = vec![
        ("", "empty"),
        (".", "reserved"),
        ("..", "reserved"),
        ("has space", "not allowed"),
        ("has/slash", "not allowed"),
        ("what?", "not allowed"),
        (".hidden", "must start with"),
        ("-leading", "must start with"),
    ];
    for (name, expect) in cases {
        match validate_name(name) {
            Err(GhError::InvalidName { reason, .. }) => assert!(
                reason.contains(expect),
                "{name:?}: expected reason containing {expect:?}, got {reason:?}"
            ),
            other => panic!("{name:?} should be rejected, got {other:?}"),
        }
    }
}

#[test]
fn an_over_long_name_is_rejected() {
    let long = "a".repeat(101);
    assert!(matches!(
        validate_name(&long),
        Err(GhError::InvalidName { .. })
    ));
    assert!(validate_name(&"a".repeat(100)).is_ok());
}

#[test]
fn a_path_traversal_attempt_is_rejected_as_a_name() {
    // Even though nothing is shell-interpreted, a name containing separators
    // would put the clone somewhere unexpected.
    for name in ["../escape", "..\\escape", "a/b"] {
        assert!(
            validate_name(name).is_err(),
            "{name:?} must not be accepted"
        );
    }
}

// ---------------------------------------------------------------------------
// destination checks
// ---------------------------------------------------------------------------

#[test]
fn an_existing_destination_is_refused_rather_than_overwritten() {
    let parent = temp_parent("exists");
    std::fs::create_dir_all(parent.join("taken")).unwrap();

    let repo = NewRepo::private("taken", &parent);
    match repo.validate() {
        Err(GhError::DestinationExists { path }) => assert!(path.contains("taken")),
        other => panic!("expected DestinationExists, got {other:?}"),
    }
}

#[test]
fn a_missing_parent_folder_is_refused() {
    let missing = std::env::temp_dir().join("herdup-gh-no-such-parent-xyz");
    let _ = std::fs::remove_dir_all(&missing);
    let repo = NewRepo::private("thing", &missing);
    assert!(matches!(
        repo.validate(),
        Err(GhError::DestinationParentMissing { .. })
    ));
}

#[test]
fn a_valid_request_into_a_free_destination_passes() {
    let parent = temp_parent("free");
    let repo = NewRepo::private("brand-new", &parent);
    repo.validate().expect("should be valid");
}

#[test]
fn validation_rejects_a_bad_name_before_looking_at_the_filesystem() {
    // Order matters: a bad name should not report a filesystem problem.
    let missing = std::env::temp_dir().join("herdup-gh-also-missing-xyz");
    let _ = std::fs::remove_dir_all(&missing);
    let repo = NewRepo::private("bad name", &missing);
    assert!(matches!(repo.validate(), Err(GhError::InvalidName { .. })));
}
