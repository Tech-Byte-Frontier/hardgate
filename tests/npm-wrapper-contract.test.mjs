// Contract checks for npm launcher environment, child status, and signals.
"use strict";
import assert from "node:assert/strict";
import {
  loadLauncher,
  makeTempDir,
  path,
  repoRoot,
  runLauncher,
  writeExecutable,
} from "./support/npm-wrapper/helpers.mjs";

const root = repoRoot(import.meta.url);
const launcherFile = path.join(root, "npm/hardgate/bin/hardgate.js");
const launcher = loadLauncher(launcherFile);

// K. Invalid or negative depth values are sanitized before the fuse check.
{
  const saved = process.env.HARDGATE_LAUNCHER_DEPTH;
  try {
    for (const [value, expected] of [
      [undefined, 0],
      ["0", 0],
      ["3", 3],
      ["-1", 0],
      ["abc", 0],
      ["", 0],
    ]) {
      if (value === undefined) delete process.env.HARDGATE_LAUNCHER_DEPTH;
      else process.env.HARDGATE_LAUNCHER_DEPTH = value;
      assert.equal(launcher.launcherDepth(), expected, `launcherDepth(${value})`);
    }
  } finally {
    if (saved === undefined) delete process.env.HARDGATE_LAUNCHER_DEPTH;
    else process.env.HARDGATE_LAUNCHER_DEPTH = saved;
  }
  console.log("K: launcherDepth sanitization -- OK");
}

// L. Child options inherit stdio and advance the recursion depth.
{
  const saved = process.env.HARDGATE_LAUNCHER_DEPTH;
  try {
    process.env.HARDGATE_LAUNCHER_DEPTH = "3";
    const options = launcher.spawnOptions();
    assert.equal(options.stdio, "inherit");
    assert.equal(options.windowsHide, true);
    assert.equal(options.env.HARDGATE_LAUNCHER_DEPTH, "4");
    assert.equal(
      options.env.HARDGATE_LAUNCHER_DEPTH,
      String(launcher.launcherDepth() + 1),
    );
  } finally {
    if (saved === undefined) delete process.env.HARDGATE_LAUNCHER_DEPTH;
    else process.env.HARDGATE_LAUNCHER_DEPTH = saved;
  }
  console.log("L: spawn contract -- OK");
}

// M. An override is executed directly and its ordinary numeric status is
// propagated unchanged, including a nonzero status.
{
  const dir = makeTempDir("hg-exit-");
  const script = writeExecutable(
    dir,
    "status.sh",
    "#!/bin/sh\nprintf 'override-ok:%s\\n' \"$1\"\nexit 23\n",
  );
  const res = runLauncher(launcherFile, ["probe"], {
    env: { HARDGATE_BINARY: script },
  });
  assert.equal(res.status, 23);
  assert.equal(res.signal, null);
  assert.equal(res.stdout.trim(), "override-ok:probe");
  console.log("M: override and numeric exit propagation -- OK");
}

// N. On POSIX, a child signal is re-sent to the wrapper; if signal delivery
// is unavailable (for example on Windows), the launcher still exits nonzero.
if (process.platform !== "win32") {
  const dir = makeTempDir("hg-signal-");
  const script = writeExecutable(dir, "signal.sh", "#!/bin/sh\nkill -TERM $$\n");
  const res = runLauncher(launcherFile, [], {
    env: { HARDGATE_BINARY: script },
  });
  assert.equal(res.status, null);
  assert.equal(res.signal, "SIGTERM");
  console.log("N: signal propagation -- OK");
}

// O. A missing override uses a synchronous diagnostic and a deterministic
// nonzero exit instead of throwing or silently succeeding.
{
  const missing = path.join(makeTempDir("hg-missing-"), "hardgate");
  const res = runLauncher(launcherFile, ["--version"], {
    env: { HARDGATE_BINARY: missing },
  });
  assert.equal(res.status, 1);
  assert.match(res.stderr, /Binary not executable/);
  console.log("O: missing override diagnostic -- OK");
}
