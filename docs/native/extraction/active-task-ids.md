# Active task IDs — P8-05 slice 19

Status: **slice verified**, 2026-10-08. This is one command's
placement wiring, not cutover acceptance or a terminal write-authority change.

## Existing owners and wire

- The production host's task-runtime registration already serves
  `terminal.get_active_task_ids` with `{}` and a bare array of strings.
  `TaskRegistry::live_task_ids()` supplies sorted unique IDs from live
  generations, excluding shell/SSH namespaces.
- The legacy Tauri `get_active_task_ids` command reads valid task IDs from
  `child_handles`. Multiple generations can yield duplicate IDs and the map
  does not promise order. That Embedded behavior must not be silently replaced
  with a registry lookup.
- `src/state/app/useAppStartup.ts` already consumes the result as a `Set` for
  startup interrupted/detached normalization. An External client must use the
  host's list, not its own typically empty table.

## Placement boundary

The command uses the established generic Runtime/AppHandle pattern and the
existing `host_dispatch::dispatch`. Only the selected owner's result is decoded
into `Vec<String>`. The original Embedded body stays in a deferred future.
Missing method, disconnection, absent session or a malformed response must
return an error, never a local list or an empty default.

Tests call the actual Tauri command with `tauri::test::mock_app()`, managed
TaskManager/CoreGateway state and a token-authenticated scratch ScriptedHost.
Inert Child fixtures populate local map keys without creating processes.
Generic dispatcher tests and static wiring scans are supporting evidence,
not substitutes for invoking this command. The CLI smoke uses its existing
fake-long task to exercise active and retired states against the real host.

No host method or coverage numerator is added. No actual HOME, real agent,
credentials, remote host or production data is accessed; Embedded/platform
writer and control-lease policy remain unchanged.

## Red regression

Before the command was modified, the six actual-command tests ran with rc101:
one Embedded control passed, and all five External result/refusal cases failed.
The old command returned `["local-only"]` even with a connected host, a missing
method, a disconnect, malformed host replies or no session. The test app had
already completed a real token-authenticated UDS handshake; this was a command
behavior failure, not a build failure or a test of a separate routing helper.
Evidence: `/tmp/aeroric-active-tasks-red.b40giD/command-red.log`.

## Verified result and limits

The same six command tests now pass. Fresh Tauri check, host registry control,
ownership scan and rebuilt CLI smoke passed; the real hosted fake task appeared
while running and disappeared after retirement. Final gates: cutover 17/98,
skeleton 24/188, tauri-adapter 12/1280 (lib 1219), host all 88, targeted startup
React 3 files/55 tests with zero skipped. Both fmt/clippy, desktop/mobile
typecheck, 11 command pins, paired floors, inventory/freeze, 12 contracts and
16 digest shards passed. Runtime-only injection left the protocol files unchanged.
Coverage remains 60/641, with 30 actual dispatch sites.

Independent static review found no substantiated issue. These are direct Rust
commands using MockRuntime, not dynamic WebView IPC or packaged-app tests.
Full Vitest/Swift packages and cross-platform builds were not rerun in this slice.
Logs: `/tmp/aeroric-active-tasks-targeted.A3YN9i`,
`/tmp/aeroric-active-tasks-integration.VDYnu7`,
`/tmp/aeroric-active-tasks-web.AzX1Ja`. No production cutover is authorized.
