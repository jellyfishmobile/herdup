# herdup

A desktop launcher for [herdr](https://herdr.dev) teams.

herdr is a terminal-native agent multiplexer. herdup is a small Windows/macOS app
that stands up a whole multi-agent team in it from a template — pick a project,
pick a team shape, and get a herdr workspace where every pane is running the right
CLI with the right flags and already knows what its job is.

## What it does

- **Team templates.** Solo, Duo, Squad (PM + 2 coders + QA), Full team (adds
  BuildMaster and Researcher). All editable TOML.
- **Roles that mean something.** Each pane gets a briefing, so a QA agent starts
  as QA. The coordinator is handed the roster and herdr's own CLI, so it can
  actually read and drive the other panes.
- **Preflight.** Detects missing agent CLIs before launch, and routes CLIs that
  are installed but not signed in through a sign-in stage — so a role briefing is
  never typed into a login prompt.
- **New projects.** Creates a private or public GitHub repo via `gh`, clones it,
  and launches a team into it.

## Status

Design and implementation plan are written; code has not started.

- [Design spec](docs/superpowers/specs/2026-09-02-herdup-design.md)
- [Implementation plan](docs/superpowers/plans/2026-09-02-herdup-plan.md)

Phase 0 of the plan is a blocking spike that verifies the design's central
assumption against a real herdr install. It has not been run yet.

## Design notes

herdup does not modify herdr, and does not install or update it — herdr already
ships its own installer and updater. Every operation goes through herdr's existing
public CLI.

Readiness detection is delegated to herdr rather than reimplemented: herdr ships
detection manifests for 18 agent CLIs and exposes the result as
`herdr wait agent-status <pane> --status idle`. A briefing is only ever sent to a
pane reporting `idle`, which is what makes it safe.

GitHub access shells out to `gh`. herdup registers no OAuth app and stores no
token of its own.

## Prerequisites

- [herdr](https://herdr.dev) (Windows support is preview-only beta)
- [`gh`](https://cli.github.com) — only for the new-repo flow
- Whichever agent CLIs your templates reference

## License

TBD
