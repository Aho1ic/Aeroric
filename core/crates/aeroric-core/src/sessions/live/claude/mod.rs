//! Claude Code through its stream-json control protocol (ADR-043, N5): one
//! `claude` process per session, driven the way the Claude Agent SDK drives it.

pub mod control;
pub mod map;
