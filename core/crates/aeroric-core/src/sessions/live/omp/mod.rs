//! omp (oh-my-pi) over `omp --mode rpc` (ADR-043 phase two): newline JSON on
//! stdio. [`rpc`] frames and correlates commands, [`map`] turns events into
//! session updates, [`approvals`] recognises tool approvals, [`driver`] owns
//! one omp process per session like the Claude driver.

pub mod approvals;
pub mod driver;
pub mod map;
pub mod rpc;
