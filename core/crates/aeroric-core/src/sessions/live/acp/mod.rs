//! Agent Client Protocol sessions (Q-R5, 2026-11-13): one driver for every
//! agent that speaks ACP over stdio JSON-RPC. What the protocol leaves to each
//! agent — how it starts, how Aeroric's permission tier is pinned, what its
//! tools are called — sits in one [`dialect`] row per agent. [`map`] turns
//! `session/update` notifications into session updates; [`driver`] owns one
//! agent process per session, like omp.

pub mod dialect;
pub mod driver;
pub mod map;
