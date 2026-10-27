<p align="center">
  <img src="docs/images/logo.png" alt="Aeroric Logo" width="150" />
</p>

<h1 align="center">Aeroric</h1>

<p align="center">
Native macOS workspace for AI coding agents, with an iOS companion for remote control.
</p>

<p align="center">
  <a href="./README_ZH.md">中文文档</a>
</p>

---

> **Status:** under active development. The legacy Tauri/React client has been removed;
> only the native SwiftUI clients and the Rust core remain.
>
> **Test policy (2026-10-19):** real-window UI tests are paused and test VMs are retired.
> See [the UI automation pause note](.agents/notes/implemented/process/2026-10-19-ui-automation-pause.md).

## What it is

Aeroric runs Claude Code, Codex, oh-my-pi (omp), DeepSeek Harness and custom agents
on your Mac through their structured protocols (Claude stream-json, `codex app-server`,
`omp --mode rpc`), with live messages, tool calls and inline approvals. It adds task
tracking, Git and file views, managed agent homes, account/model configuration,
and an iOS companion that observes tasks, answers approvals and receives push
notifications. All execution stays on the Mac.

## Repository layout

```
core/            Rust workspace: aeroric-protocol, aeroric-core, aeroric-host, aeroric-cli
  vendor/        Patched third-party crates (portable-pty, ssfmt, notify-types)
macos/           SwiftUI macOS app (AeroricApp) and AeroricKit package
ios/             SwiftUI iOS companion and AeroricCompanionKit package
contracts/       Wire contracts and golden fixtures shared by Rust and Swift
remote-protocol/ Relay control-plane protocol
remote-push/     APNs sender
remote-relay/    Self-hostable relay
scripts/native/  Build, signing, gate runner and UI-test tooling (Node, no npm deps)
docs/            Architecture, subsystem docs, cookbook and backlog (start at docs/architecture.md)
  native/        Machine-read inventories used by gates and generators
.agents/notes/   Decision notes (why things are the way they are)
```

`core` depends on the external [dbx](https://github.com/t8y2/dbx) checkout at `../dbx`,
pinned by `scripts/dbx-ref.txt` and patched by `scripts/prepare-dbx.sh`.

## Development

Requirements: macOS 26+, Xcode, Rust (see `core/Cargo.toml` `rust-version`), Node 24.

```bash
pnpm native:doctor                 # check toolchain
pnpm native:build                  # build and locally sign AeroricNative.app
pnpm native:check <module>         # run a gate module from scripts/native/checks.json

cargo test --manifest-path core/Cargo.toml --workspace
swift test --package-path macos/AeroricKit
swift test --package-path ios/AeroricCompanionKit
```

## Distribution

- macOS: Developer ID signing and notarization, distributed outside the Mac App Store.
- iOS: App Store, as a remote-control companion for the user's own paired Mac.

See [docs/cookbook/release-and-signing.md](docs/cookbook/release-and-signing.md).

## License

Proprietary. See [LICENSE](./LICENSE). Vendored third-party crates keep their own licenses.
