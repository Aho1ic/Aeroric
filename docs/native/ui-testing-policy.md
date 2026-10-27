# Project-wide UI pause and VM retirement

Effective **2026-10-19, Asia/Shanghai**, by the user's explicit instruction to
free the two Aeroric VMs, pause UI tests project-wide, and prevent recreation.
This operational notice supersedes earlier VM/UI runtime permissions, not
product requirements or historical evidence.

## Current execution policy

- Real-window UI automation is **PAUSED / DEFERRED**. Do not launch a full App,
  prewarm it, synthesize input, run XCUITest or invoke an equivalent private
  host/guest runner. This includes `pnpm native:ui-test` and
  `pnpm native:check accessibility`. (The single user-authorized manual
  inspection launch recorded below is the only exception; it does not resume
  automation.)
- Test VM provisioning and reuse are **FORBIDDEN**: no create, clone, import,
  pull/download, restore, boot, VM tooling reinstallation, or automatic repair
  of paths removed by retirement. Old task reservations and approvals expire
  for future UI/VM execution; `.artifacts/` helpers are historical evidence.
- The official UI runner must refuse **before** resources, compilation,
  prewarm, App launch or test-log replacement, with nonzero exit **78**.
  A policy refusal is not a test pass, skip-to-green or completed acceptance.
  The gate is in `scripts/native/ui-test-policy.{json,mjs}`; no environment or
  command-line bypass is authorized.
- Continue only checks reviewed to avoid real windows, user input and real
  business/agent data: targeted unit/fixture/static checks and compile-only
  builds. Use fake agents and independent temporary roots. An offscreen
  `NSHostingView` proves view assembly, not real Liquid Glass, AX, keyboard,
  window geometry, screenshots or S1–S8 user-journey acceptance.
- Do not weaken floors, remove UI cases/assertions, change product behavior,
  or label old acceptance gaps resolved because this policy pauses execution.
- Resume real UI or replacement VM infrastructure only after **new explicit
  user authorization**, resource/permission review and a reviewed policy
  update. This retirement does not authorize a replacement account, remote
  access, CI purchase, system/TCC change, commit, push or publication.

## 2026-10-20 scoped authorization: one manual inspection launch

The user explicitly asked, in the current project conversation, to launch the
current half-finished native app so they can look at its UI. That request is
the "new explicit user authorization" this notice requires; the review and this
policy update complete the three conditions for resuming real UI. Everything
else in this notice stays in force.

- **Scope**: one manual launch, initiated by the agent, of the already-built
  DEBUG bundle `.artifacts/native/DerivedData/Build/Products/Debug/AeroricNative.app`
  (ad-hoc signed, built by `pnpm native:build` on 2026-10-20) for the user's own
  visual inspection on this workstation. No input synthesis, no automation, no
  screenshots for acceptance, no test-runner involvement.
- **Resource/permission review (2026-10-20)**: the product path resolves its
  data root as `AERORIC_DATA_DIR` when injected, otherwise
  `~/Library/Application Support/.aeroric` (absent at review time; a plain
  launch creates it); `AppServices.startSupervisor()` then starts the bundled
  `aeroric-cli` host child with a `<dataDir>/host.sock` UDS. The app layer has
  no network listener, no entitlements and no App Sandbox; the bundle is ad-hoc
  signed. The `AERORIC_TEST_OPEN_PROJECT` pkill hook is not set. No VM is
  involved and no VM/UI-runner resource is touched.
- **Still forbidden**: VM provisioning/reuse, XCUITest, `pnpm native:ui-test`,
  `pnpm native:check accessibility`, input synthesis/automation, prewarming for
  tests, TCC/system changes, and any further launch that the user has not
  explicitly requested in the current conversation.
- **Evidence**: `.artifacts/native/task-communication-review/` run directory for
  the launch (command, environment, process state, app stdout/stderr).
- **Further real-UI runs** (including any automation or a second app/sample)
  need a fresh explicit authorization and their own resource/permission review.

## Retirement scope and evidence

Only these two dedicated roots are in the deletion scope:

```text
~/Library/Application Support/AeroricNativeTestVMs/stage01-20261015/
~/Library/Application Support/AeroricNativeTestVMs/skill-review-20261018-150848/
```

The user authorizes removing their system disks, NVRAM/configuration, restore
images, VM payloads, and large offline/maintenance copies. Shut down the running
guest normally, verify the exact controller/Tart exits, and check mounts/open
files before removing data. Do not use `tart stop` with a force-stop fallback,
kill a holder, or remove locks to gain access. Retain compact ownership
tombstones and metadata/receipts; they do not reserve a future run.

Keep project `.artifacts/` reports, raw UI results, screenshots/AX packets,
source snapshots and task histories. Export any additional necessary guest
evidence before shutdown/deletion, without starting another test. No product
source, real agent home/credentials or unrelated VM is a cleanup candidate.

**Completed at 2026-10-19 11:48 +08:** both VM infrastructures and their large
backup/maintenance copies were removed after verified normal shutdown. The
remaining namespace is44 KiB of ownership/tombstone records, with parent and
two retired roots mode0500; ordinary directory-creation probes were denied.
This is an accidental-recreation barrier, **not** a security boundary against
an owner/admin changing permissions or choosing another VM location.
The point-in-time filesystem free-space increase was306,851,237,888 bytes
(285.78 GiB), with about389.54 GiB available at post-verification. The earlier
634.70 GiB `du` total was not a reclaimable-space promise.

Current maintenance record:
`.trellis/tasks/10-18-native-skill-review/research/ui-pause-vm-retirement-2026-10-19.md`.
That record and its receipts, not a planned deletion, determine actual released
space and remaining work. APFS clone/snapshot sharing means directory `du`
counts are not a promise of the same increase in filesystem free space.

## Future UI without a local VM (not authorized to run now)

1. **Dedicated physical/remote Mac with a logged-in test desktop:** Run real
   XCUITest there, export xcresult/PNG/AX and preserve the original gate. Input
   stays on that Mac, not this workstation. A self-hosted CI controller can
   schedule it; it still needs a usable GUI session and reviewed signing,
   automation permissions, data isolation and remote-access security.
2. **Separate macOS account plus a genuinely separate Remote Desktop virtual
   login display:** Apple documents a different authenticated user's desktop
   without interrupting the console user. This is a candidate, not proof that
   current macOS/XCTest works unattended in that session. Validate session
   routing, AX/input/screen-capture permissions, screenshots, focus, connection
   loss and logout using a small separately approved probe. Do not confuse
   “share the console display” with a separate virtual login display.
3. **Offscreen component/hosting/snapshot tests:** Useful for fast background
   view/state/layout regressions without input automation. They cannot replace
   real compositor/material, keyboard/focus, menus, VoiceOver or end-to-end
   acceptance. Current windowless Skill tests are an example.

Another Space, a second monitor, `nohup`, SSH, `launchctl asuser`, hidden
windows or unreviewed input injection do **not** constitute a demonstrated
separate desktop. Do not promise no pointer/focus interference from them.

Primary-source references checked for this notice: Apple Developer
`XCUIApplication.state` (successful launch/activate normally means foreground);
Apple Remote Desktop “Choose how to control and observe” (different-account
virtual display); Apple Developer “Reducing your app's disk usage” (APFS clones).
Exact source references are retained in the retirement report.
