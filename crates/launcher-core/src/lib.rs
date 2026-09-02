//! herdup core.
//!
//! Phase 1 delivers [`herdr`], the typed wrapper over the `herdr` binary.
//! Later phases add the registry, templates, plan generation and the executor.
//!
//! Design: `docs/superpowers/specs/2026-09-02-herdup-design.md`

pub mod herdr;

pub use herdr::{HerdrCli, HerdrError};
