// Negative controls for scripts/native/adapter-invariants.mjs.
//
// An absence-style guard that cannot be made to fire is worse than no guard: it reports "clean"
// for an empty corpus, a renamed file or a broken scanner. Each case here mutates one fact and
// requires the specific problem text, and one case proves the scan refuses to judge what it could
// not read.
import assert from "node:assert/strict";
import test from "node:test";

import { adapterInvariantProblems, enclosingFunction, rustFiles } from "./adapter-invariants.mjs";

const PTY = "src-tauri/src/pty.rs";
const LIB = "src-tauri/src/lib.rs";
const ADAPTER = "src-tauri/src/core_adapter.rs";
const SESSION = "src-tauri/src/session.rs";

/** A source tree that satisfies all three rules, so every mutation below is attributable to the
 *  one line it changes rather than to scaffolding. */
function cleanFiles() {
  return [
    {
      name: PTY,
      text: `pub(crate) static STARTUP_SIGNAL_VERSION: AtomicU64 = AtomicU64::new(1);
pub(crate) struct StartupSignalRegistration { version: u64, sender: Sender }
pub(crate) fn register_initial_input_signal(task_manager: &TaskManager, run: &RunKey) -> u64 {
    let version = STARTUP_SIGNAL_VERSION.fetch_add(1, Ordering::Relaxed);
    version
}
pub(crate) fn clear_initial_input_signal_if_current(signals: &StartupSignalRegistry) {
    let _ = STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed);
}
`,
    },
    {
      name: LIB,
      text: `impl TaskManager {
    pub(crate) fn release_session_path_if_unreferenced(&self, path: &str) -> bool {
        let referenced = false;
        if referenced {
            return false;
        }
        self.claimed_session_paths.lock().remove(&path.to_string())
    }
}
`,
    },
    {
      name: ADAPTER,
      text: `pub(crate) fn writer_handover_allowed() -> bool {
    cfg!(target_os = "macos")
}
`,
    },
    { name: SESSION, text: `fn claim_session_path(app: &AppHandle, path: &str) -> bool {\n    true\n}\n` },
  ];
}

function problemText(files) {
  return adapterInvariantProblems(files).join("\n");
}

test("the reference tree satisfies every rule", () => {
  assert.deepEqual(adapterInvariantProblems(cleanFiles()), []);
});

test("a second run-identity source is refused", () => {
  const files = cleanFiles();
  files[0].text += "\nstatic STARTUP_SIGNAL_GENERATION: AtomicU64 = AtomicU64::new(1);\n";
  assert.match(problemText(files), /STARTUP_SIGNAL_GENERATION is back/);
});

test("the version stamp cannot leak into another file", () => {
  const files = cleanFiles();
  files[3].text += "\nlet _ = STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed);\n";
  assert.match(problemText(files), /read outside pty\.rs/);
});

test("and cannot be read by a third function in pty.rs either", () => {
  const files = cleanFiles();
  files[0].text += `pub(crate) fn some_other_thing() -> u64 {
    STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed)
}
`;
  assert.match(problemText(files), /also read by some_other_thing/);
});

test("a stamp nothing reads means the rule is not being checked", () => {
  const files = cleanFiles();
  files[0].text = files[0].text
    .replace("    let version = STARTUP_SIGNAL_VERSION.fetch_add(1, Ordering::Relaxed);\n", "")
    .replace("    let _ = STARTUP_SIGNAL_VERSION.load(Ordering::Relaxed);\n", "");
  assert.match(problemText(files), /no function reads STARTUP_SIGNAL_VERSION/);
});

test("a second claim-removal site is refused", () => {
  const files = cleanFiles();
  files[3].text = files[3].text.replace(
    "    true\n}",
    `    let mut claimed = tm.claimed_session_paths.lock();
    claimed.remove(&path);
    true
}`,
  );
  assert.match(problemText(files), /claim removal outside the reverse-lookup helper: src-tauri\/src\/session\.rs:claim_session_path/);
});

test("a helper that no longer removes anything is caught, not praised", () => {
  const files = cleanFiles();
  files[1].text = files[1].text.replace(
    "        self.claimed_session_paths.lock().remove(&path.to_string())",
    "        let _ = path;\n        false",
  );
  assert.match(problemText(files), /contains no claim removal/);
});

test("handover must stay macOS-only", () => {
  const always = cleanFiles();
  always[2].text = always[2].text.replace('    cfg!(target_os = "macos")\n', "    true\n");
  assert.match(problemText(always), /no longer returns cfg!\(target_os = "macos"\)/);

  const widened = cleanFiles();
  widened[2].text = widened[2].text.replace(
    '    cfg!(target_os = "macos")\n',
    '    cfg!(any(target_os = "macos", target_os = "windows"))\n',
  );
  assert.match(problemText(widened), /names windows\/linux|no longer returns/);
});

test("an unreadable guard fails instead of passing", () => {
  const noHelper = cleanFiles();
  noHelper[1].text = "impl TaskManager {}\n";
  assert.match(problemText(noHelper), /the scan read nothing/);

  const noAdapter = cleanFiles();
  noAdapter[2].text = "// rewritten\n";
  assert.match(problemText(noAdapter), /the scan read nothing/);
});

test("methods are attributed to their own function, not to the last free fn", () => {
  const lib = cleanFiles()[1].text.split("\n");
  assert.equal(enclosingFunction(lib, 1), "release_session_path_if_unreferenced");
  const withPrecedingFreeFn = `pub(crate) fn authorize_app_exit() {}\n\nimpl TaskManager {\n    pub(crate) fn helper(&self) {}\n}\n`.split("\n");
  assert.equal(enclosingFunction(withPrecedingFreeFn, 3), "helper");
});

test("the real tree is scanned, not an empty stand-in", () => {
  const names = rustFiles("src-tauri/src");
  assert.ok(names.length > 100, `expected the real src-tauri tree, got ${names.length} files`);
  assert.ok(names.includes(PTY) && names.includes(LIB) && names.includes(ADAPTER));
});
