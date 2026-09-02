//! herdup core.
//!
//! - [`herdr`] — typed wrapper over the `herdr` binary (Phase 1)
//! - [`registry`] — known CLIs: binary, install hints, flags, briefing trust (Phase 2)
//! - [`template`] — team templates: roles, layout, briefings (Phase 2)
//! - [`plan`] — pure template-to-operations planning (Phase 3)
//!
//! Later phases add the executor, preflight, and the GUI.
//!
//! Design: `docs/superpowers/specs/2026-09-02-herdup-design.md`

pub mod config;
pub mod execute;
pub mod herdr;
pub mod plan;
pub mod registry;
pub mod template;

pub use config::ConfigError;
pub use herdr::{HerdrCli, HerdrError};
pub use plan::{plan, BriefingGate, LaunchPlan, LaunchRequest, PaneRef, Step};
pub use registry::{BriefingTrust, CliEntry, Registry};
pub use template::{PaneSpec, Template, Templates};
