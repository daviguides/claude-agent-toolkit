//! Bidirectional control protocol on top of [`crate::transport::Transport`].
//!
//! Nothing outside `#[cfg(test)]` calls into this module yet — Phase 6
//! (`query()`) and Phase 7 (`ClaudeClient`) are its first real
//! callers. Until then every item here is legitimately unused from the
//! plain `--lib` build's point of view, hence the blanket allow; remove
//! it once Phase 6 wires this module into the public API.
#![allow(dead_code)]

pub(crate) mod control;
pub(crate) mod query;
