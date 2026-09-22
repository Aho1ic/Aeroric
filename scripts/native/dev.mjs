#!/usr/bin/env node
// `pnpm dev` — deliberately NOT READY.
// MIGRATION_PLAN.md P1-04 Action 5: a not-yet-implemented action must exit non-zero
// and say so. An always-exit-0 placeholder is explicitly forbidden by §3.2, and a
// green stub here would let a later phase believe a gate had passed.
import process from "node:process";

console.error("dev.mjs: NOT READY — implemented by Task P4-01.");
console.error("  Reason: opens a Debug app that does not exist yet; would silently launch nothing.");
console.error("  Refusing to exit 0, because that would read as a passing check.");
process.exit(78); // EX_CONFIG
