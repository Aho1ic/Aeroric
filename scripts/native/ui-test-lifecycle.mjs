import path from "node:path";

export const nativeProcessNames = new Set([
  "AeroricNative", "aeroric-cli", "aeroric-host", "xcodebuild", "xctest",
  "AeroricNativeUITests-Runner",
]);

export function resources(home, runID) {
  if (!/^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(runID)) {
    throw new Error("invalid UI run identity");
  }
  const root = path.join(home, "Library/Caches/aeroric-native", `ui-test-${runID}`);
  // XCTest writes only inside its existing sandbox, never by gaining account
  // Caches write access. The outer runner creates one new receipt root there.
  const receipts = path.join(home, "Library/Containers/com.aeroric.native.dev.uitests.xctrunner/Data/Library/Caches",
    `aeroric-ui-lifecycle-${runID}`);
  return { runID, root, scratch: path.join(root, "data"), warmup: path.join(root, "warmup"),
    receipts, failure: path.join(receipts, "lifecycle-failure.json") };
}

export function assertQuiescent(sample) {
  if (sample?.schemaVersion !== 1 || !Array.isArray(sample.errors) || sample.errors.length
      || !Array.isArray(sample.applications) || !Array.isArray(sample.processes)
      || sample.applications.length || sample.processes.length) {
    throw new Error("native UI resources are occupied or observation failed; no cleanup authorized");
  }
}

export function assertAttemptComplete(receipt, runID, attemptID) {
  if (receipt?.version !== 1 || receipt.runID !== runID || receipt.attemptID !== attemptID
      || receipt.safeToRetry !== true) {
    throw new Error("ordinary test has no verified lifecycle completion; no retry or next method");
  }
}

// Node has no openat/mkdirat API. This tiny syscall adapter binds every
// mutation to a verified, no-follow parent descriptor. It is not a controller,
// never inspects processes/preferences, and is inert until the injected exec.
export function descriptorIO(exec) {
  const program = `
import json,os,stat,sys
v=json.load(sys.stdin)
p=v["path"];assert os.path.isabs(p) and os.path.normpath(p)==p
parent,name=os.path.split(p);assert name not in {"",".",".."}
fd=os.open("/",os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
try:
 for part in filter(None,parent.split("/")):
  child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=fd)
  os.close(fd);fd=child
 s=os.fstat(fd);assert [s.st_dev,s.st_ino]==v["parent"]
 if v["operation"]=="mkdir":
  os.mkdir(name,0o700,dir_fd=fd)
  child=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=fd)
  try:
   s=os.fstat(child);assert s.st_uid==os.getuid()
   print(json.dumps({"dev":s.st_dev,"ino":s.st_ino}))
  finally:os.close(child)
 else:
  flags=os.O_NOFOLLOW|(os.O_RDONLY if v["operation"]=="read" else os.O_WRONLY|os.O_CREAT|os.O_EXCL)
  child=os.open(name,flags,0o600,dir_fd=fd)
  try:
   s=os.fstat(child);assert stat.S_ISREG(s.st_mode) and s.st_uid==os.getuid()
   if v["operation"]=="read":
    assert s.st_size<=65536
    with os.fdopen(os.dup(child),"r") as source: print(json.dumps(source.read()))
   else:
    data=v["value"].encode("utf-8");assert os.write(child,data)==len(data);os.fsync(child)
  finally:os.close(child)
 os.fsync(fd)
finally:os.close(fd)
`;
  function operation(operation, candidate, parent, value) {
    if (!Array.isArray(parent) || parent.length !== 2) throw new Error("missing pinned parent identity");
    const result = exec("/usr/bin/python3", ["-I", "-B", "-c", program], {
      input: JSON.stringify({ operation, path: candidate, parent, value }), encoding: "utf8",
    });
    if (result.error || result.status !== 0) throw new Error("descriptor-relative UI resource operation failed");
    return operation === "read" || operation === "mkdir" ? JSON.parse(result.stdout) : undefined;
  }
  return {
    mkdir: (candidate, recursive, parent) => {
      if (recursive) throw new Error("recursive resource creation forbidden");
      return operation("mkdir", candidate, parent);
    },
    write: (candidate, value, parent) => operation("write", candidate, parent, value),
    read: (candidate, parent) => operation("read", candidate, parent),
  };
}

/// All I/O is injected. Importing this module does not inspect or start an App.
export function createLifecycle({ home, runID, artifacts, io }) {
  const owned = resources(home, runID);
  const lease = path.join(artifacts, "ui-test-active");
  const receipt = JSON.stringify({ version: 1, runID, root: owned.root });
  let reserved = false;
  let finished = false;
  let poisoned = false;
  let lastClear = false;
  const identities = new Map();
  function safePath(candidate) {
    for (let current = candidate; ; current = path.dirname(current)) {
      const status = io.lstat(current);
      if (status && (status.symlink || !status.directory)) throw new Error("unsafe UI resource path");
      if (status) {
        if (status.dev == null || status.ino == null) throw new Error("missing directory identity");
        const known = identities.get(current);
        if (known && (status.dev !== known[0] || status.ino !== known[1])) throw new Error("UI ancestor identity changed");
        if (!known) identities.set(current, [status.dev, status.ino]);
      }
      if (current === path.dirname(current)) break;
    }
  }
  function remember(candidate) {
    safePath(candidate);
    const status = io.lstat(candidate);
    if (!status?.directory || status.dev == null || status.ino == null) throw new Error("missing directory identity");
    const known = identities.get(candidate);
    if (known && (status.dev !== known[0] || status.ino !== known[1])) throw new Error("UI directory identity changed");
    if (!known) identities.set(candidate, [status.dev, status.ino]);
  }
  function makeDirectory(candidate) {
    const born = io.mkdir(candidate, false, identities.get(path.dirname(candidate)));
    if (born) identities.set(candidate, [born.dev, born.ino]);
    remember(candidate); // Must still be the directory returned by mkdirat/openat.
  }
  function verify() {
    if (!reserved || finished || poisoned) throw new Error("UI lifecycle is not available");
    for (const [candidate, identity] of identities) {
      safePath(candidate);
      const status = io.lstat(candidate);
      if (!status?.directory || status.dev !== identity[0] || status.ino !== identity[1]) {
        throw new Error("UI resource directory identity changed");
      }
    }
    const owner = path.join(lease, "owner.json");
    const status = io.lstat(owner);
    if (!status || status.symlink || status.directory || io.read(owner, identities.get(lease)) !== receipt) {
      throw new Error("UI lease ownership changed");
    }
    if (io.lstat(owned.failure)) throw new Error("latched Swift lifecycle failure; no retry or next launch");
  }
  function ensureParents(candidate) {
    const parent = path.dirname(candidate);
    if (!io.lstat(parent)) ensureParents(parent);
    safePath(parent);
    if (!io.lstat(candidate)) {
      // No recursive mkdir: an unexpected concurrent creator is contention.
      makeDirectory(candidate);
    }
    safePath(candidate);
  }
  return {
    ...owned,
    reserve() {
      safePath(owned.root);
      safePath(owned.receipts);
      safePath(artifacts);
      if (io.lstat(owned.root)) throw new Error("UI run root already exists");
      if (io.lstat(owned.receipts)) throw new Error("UI receipt root already exists");
      ensureParents(artifacts);
      // Exclusive creation, never remove/recover somebody else's lease.
      makeDirectory(lease);
      reserved = true;
      remember(lease);
      io.write(path.join(lease, "owner.json"), receipt, identities.get(lease));
      ensureParents(path.dirname(owned.root));
      makeDirectory(owned.root);
      makeDirectory(owned.scratch);
      makeDirectory(owned.warmup);
      ensureParents(path.dirname(owned.receipts));
      makeDirectory(owned.receipts);
      for (const candidate of [owned.root, owned.scratch, owned.warmup, owned.receipts]) remember(candidate);
      verify();
    },
    reservation() {
      verify();
      return { version: 1, runID, root: owned.root, lease, ownerReceipt: receipt,
        directories: [owned.root, owned.scratch, owned.warmup, owned.receipts, lease]
          .map((candidate) => ({ path: candidate, dev: identities.get(candidate)[0], ino: identities.get(candidate)[1] })) };
    },
    async barrier() {
      try {
        verify();
        assertQuiescent(await io.snapshot(owned));
        verify();
        lastClear = true;
      } catch (error) {
        poisoned = true;
        lastClear = false;
        throw error; // Keep the lease and identity for evidence, not another launch.
      }
    },
    async warmup() {
      try {
        verify();
        assertQuiescent(await io.snapshot(owned));
        verify();
        lastClear = false;
        const result = await io.warmup(owned);
        if (result?.completed !== true || result?.causalLaunchReceipt !== true || result?.exitClean !== true) {
          throw new Error("owned LaunchServices warmup did not normally quit");
        }
        assertQuiescent(await io.snapshot(owned));
        verify();
        lastClear = true;
      } catch (error) {
        poisoned = true;
        throw error;
      }
    },
    finish() {
      verify();
      if (!lastClear) {
        throw new Error("UI lease cannot be released");
      }
      // Retain the lease in place with a completion receipt. No rename/unlink
      // race can move another owner's replacement directory. A later run must
      // obtain an explicit resource handoff, never auto-recover this lease.
      io.write(path.join(lease, "completed.json"), receipt, identities.get(lease));
      verify();
      finished = true;
    },
  };
}
