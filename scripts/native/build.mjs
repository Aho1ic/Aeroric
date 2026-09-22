#!/usr/bin/env node
// `pnpm build` — deliberately NOT READY.
// MIGRATION_PLAN.md P1-04 Action 5: a not-yet-implemented action must exit non-zero
// and say so. An always-exit-0 placeholder is explicitly forbidden by §3.2, and a
// green stub here would let a later phase believe a gate had passed.
import process from "node:process";

console.error("build.mjs: NOT READY — implemented by Task P4-01.");
console.error("  Reason: Rust host build → bundle copy → xcodebuild → inside-out ad-hoc signing → verification is wired in P4-01. Half of it (the aeroric-cli binary) only reaches a real command tree in P3-07.");
console.error("  Refusing to exit 0, because that would read as a passing check.");
process.exit(78); // EX_CONFIG
