#!/usr/bin/env node
// `pnpm contracts` — deliberately NOT READY.
// MIGRATION_PLAN.md P1-04 Action 5: a not-yet-implemented action must exit non-zero
// and say so. An always-exit-0 placeholder is explicitly forbidden by §3.2, and a
// green stub here would let a later phase believe a gate had passed.
import process from "node:process";

console.error("contracts.mjs: NOT READY — implemented by Task P2-01.");
console.error("  Reason: contracts/native/v1 schemas and fixtures, the Rust export-schema binary and the three-language decoder round trip all land in P2-01.");
console.error("  Refusing to exit 0, because that would read as a passing check.");
process.exit(78); // EX_CONFIG
