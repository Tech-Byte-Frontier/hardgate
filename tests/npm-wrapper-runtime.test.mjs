// End-to-end npm launcher checks for resolution failures, recursion, and
// PATH candidate filtering.
"use strict";
import assert from "node:assert/strict";
import {
  assertMissingBinary,
  assertPathVersion,
  copyLauncherAt,
  fs,
  loadLauncher,
  makePathFixture,
  makeTempDir,
  os,
  path,
  readVersion,
  realBinary,
  repoRoot,
  runLauncher,
  scrubbedPath,
} from "./support/npm-wrapper/helpers.mjs";

const root = repoRoot(import.meta.url);
const launcher = path.join(root, "npm/hardgate/bin/hardgate.js");
const binary = realBinary(root);
const expectedVersion = readVersion(binary);
assert.match(expectedVersion, /hardgate \d+\.\d+\.\d+/);

// A. A PATH shim is rejected when no platform package or dev binary exists.
{
  const dir = makeTempDir("hg-shim-");
  const fakeBinDir = path.join(dir, "bin");
  fs.mkdirSync(fakeBinDir, { recursive: true });
  fs.writeFileSync(
    path.join(fakeBinDir, "hardgate"),
    `#!/bin/sh\nexec "${process.execPath}" "${launcher}" "$@"\n`,
    { mode: 0o755 },
  );
  const launcherCopy = copyLauncherAt(launcher, dir);
  const res = runLauncher(launcherCopy, ["--version"], {
    env: { PATH: scrubbedPath([fakeBinDir]) },
  });
  assertMissingBinary(
    res,
    loadLauncher(launcherCopy).platformPackage(),
    "shim",
  );
  console.log("A: shim-on-PATH degrades to clean exit 1 -- OK");
}

// A2. Unsupported hosts fail before Unix-only PATH/dev fallback probing.
{
  const dir = makeTempDir("hg-platform-");
  const preload = path.join(dir, "platform.cjs");
  fs.writeFileSync(
    preload,
    'Object.defineProperty(process, "platform", { value: "win32" });\n',
  );
  const res = runLauncher(launcher, ["--version"], {
    nodeArgs: ["--require", preload],
  });
  assert.equal(res.status, 1);
  assert.match(res.stderr, /Unsupported platform win32/);
  console.log("A2: unsupported platform fails clearly -- OK");
}

// B. A self override reaches the recursion fuse instead of spawning forever.
{
  const res = runLauncher(launcher, ["--version"], {
    env: { HARDGATE_BINARY: launcher },
    timeout: 8000,
  });
  assert.equal(res.status, 1, `fuse must exit 1, got ${res.status}`);
  assert.match(res.stderr ?? res.stdout, /[Rr]ecurs/);
  console.log("B: self-exec fuse trips with clear error -- OK");
}

// D. A pnpm/dlx-style consumer tree with a missing package binary must fail
// quickly, even when its .bin shim is first on PATH.
{
  const dir = makeTempDir("hg-dlx-");
  const nm = path.join(dir, "node_modules");
  const dotbin = path.join(nm, ".bin");
  fs.mkdirSync(dotbin, { recursive: true });
  const platformDir = path.join(nm, "hardgate-linux-x64");
  fs.mkdirSync(path.join(platformDir, "bin"), { recursive: true });
  fs.writeFileSync(
    path.join(platformDir, "package.json"),
    JSON.stringify({ name: "hardgate-linux-x64", version: "0.0.0-test" }),
  );
  const mainDir = path.join(nm, "hardgate/bin");
  const launcherCopy = copyLauncherAt(launcher, dir);
  fs.writeFileSync(
    path.join(dotbin, "hardgate"),
    `#!/bin/sh\nexec "${process.execPath}" "${launcherCopy}" "$@"\n`,
    { mode: 0o755 },
  );
  const res = runLauncher(launcherCopy, ["--version"], {
    cwd: os.tmpdir(),
    env: { PATH: scrubbedPath([dotbin]) },
  });
  assertMissingBinary(
    res,
    loadLauncher(launcherCopy).platformPackage(),
    "dlx shape",
  );
  assert.equal(fs.existsSync(mainDir), true);
  console.log("D: dlx-without-binary exits 1 without recursion -- OK");
}

// E. A script earlier on PATH is skipped in favor of a later real binary.
{
  const fixture = makePathFixture(launcher, binary, "hg-magic-");
  fs.writeFileSync(
    path.join(fixture.first, "hardgate"),
    "#!/bin/sh\necho SHOULD-NEVER-RUN\n",
    { mode: 0o755 },
  );
  assertPathVersion({
    launcher: fixture.launcherCopy,
    first: fixture.first,
    later: fixture.later,
    expected: expectedVersion,
    label: "script",
  });
  console.log("E: PATH script skipped in favor of real binary -- OK");
}

// J. A directory named hardgate is also skipped in favor of a real binary.
{
  const fixture = makePathFixture(launcher, binary, "hg-dirskip-");
  fs.mkdirSync(path.join(fixture.first, "hardgate"), { recursive: true });
  assertPathVersion({
    launcher: fixture.launcherCopy,
    first: fixture.first,
    later: fixture.later,
    expected: expectedVersion,
    label: "directory",
  });
  console.log("J: directory named hardgate skipped -- OK");
}
