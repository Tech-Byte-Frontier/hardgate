// Run directly (outside with-resource-limits.sh) to exercise the real supervisors.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";
import { boundary } from "../scripts/check-resource-boundary.mjs";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-concurrency-test-"));
const repository = path.resolve(import.meta.dirname, "..");
const binary = path.resolve(process.env.HARDGATE_TEST_BINARY ?? path.join(repository, "target/debug/hardgate"));
const processes = [], scopes = new Set();
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const environment = { ...process.env, XDG_RUNTIME_DIR: `/run/user/${process.getuid()}` };
delete environment.DBUS_SESSION_BUS_ADDRESS;

function run(command, args, options = {}) {
  const child = spawn(command, args, { cwd: repository, env: environment, stdio: ["ignore", "pipe", "pipe"], ...options });
  const process = { child, stdout: "", stderr: "" };
  child.stdout.on("data", (bytes) => { process.stdout += bytes; });
  child.stderr.on("data", (bytes) => { process.stderr += bytes; });
  process.done = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
  processes.push(process);
  return process;
}

async function until(predicate, message) {
  const deadline = Date.now() + 15000;
  while (!predicate()) {
    assert.ok(Date.now() < deadline, message);
    await pause(25);
  }
}

function maintenance(milliseconds) {
  const code = `const fs = require('node:fs'); console.log(JSON.stringify({scope: fs.readFileSync('/proc/self/cgroup', 'utf8').trim(), started: Date.now()})); setTimeout(() => console.log(JSON.stringify({finished: Date.now()})), ${milliseconds});`;
  return run(path.join(repository, "scripts/with-resource-limits.sh"), [process.execPath, "-e", code]);
}

async function started(job) {
  await until(() => job.stdout.includes("\n"), `workload did not start: ${job.stderr}`);
  const event = JSON.parse(job.stdout.split("\n")[0]);
  const unit = event.scope.split("/").at(-1);
  assert.match(unit, /^hardgate-workload-\d+-[\w-]+\.scope$/);
  scopes.add(unit);
  return event;
}

async function successful(job, report = false) {
  const status = await job.done;
  assert.equal(status.code, 0, `${JSON.stringify(status)}\n${job.stderr}\n${job.stdout}`);
  if (report) {
    const value = JSON.parse(job.stdout);
    assert.equal(value.partial, true);
    assert.equal(value.accepted, false);
    assert.equal(value.passed, true);
  }
}

function fixture(name) {
  const directory = path.join(root, name);
  fs.mkdirSync(path.join(directory, "src"), { recursive: true });
  fs.writeFileSync(path.join(directory, "src/index.ts"), "export const answer = 42;\n");
  fs.writeFileSync(path.join(directory, "hardgate.toml"), "[gate]\npreset='custom'\n[orchestration]\nrequire_isolation=true\ntest_cmd='sh probe.sh'\n");
  fs.writeFileSync(path.join(directory, "probe.sh"), `set -eu
slot=0
for descriptor in /proc/$$/fd/*; do
  target=$(readlink "$descriptor" || true)
  case "$target" in /tmp/hardgate-workload-*/slot.lock) slot=1;; esac
done
test "$slot" = 1
sleep "$HARDGATE_TRIAL_PAUSE"
`);
  return directory;
}

function check(directory, seconds = "0.1") {
  return run(binary, ["check", "--checks", "tests", "--json"], { cwd: directory, env: { ...environment, HARDGATE_TRIAL_PAUSE: seconds } });
}

async function ownedScope(job) {
  let unit;
  await until(() => {
    const output = execFileSync("systemctl", ["--user", "list-units", "--all", "--plain", "--no-legend", `hardgate-workload-${job.child.pid}-*.scope`], { env: environment, encoding: "utf8" });
    unit = output.trim().split(/\s+/)[0];
    return unit.endsWith(".scope");
  }, `Hardgate did not establish its owned scope: ${job.stderr}`);
  scopes.add(unit);
  return unit;
}

function inputs(directory) {
  return ["src/index.ts", "hardgate.toml", "probe.sh"].map((name) => fs.readFileSync(path.join(directory, name), "utf8"));
}

async function exercise() {
  let inherited = false;
  try { boundary(); inherited = true; } catch { /* A direct supervisor trial starts unbounded. */ }
  assert.equal(inherited, false, "run this supervisor test directly, outside an inherited resource boundary");
  const first = maintenance(600), firstStart = await started(first);
  const second = maintenance(5);
  await until(() => second.stderr.includes("waiting"), "maintenance calls did not share the workload slot");
  assert.equal(second.stdout, "");
  await successful(first);
  await successful(second);
  const secondStart = await started(second);
  const firstEnd = JSON.parse(first.stdout.trim().split("\n").at(-1));
  assert.notEqual(firstStart.scope, secondStart.scope);
  assert.ok(secondStart.started >= firstEnd.finished);

  const a = fixture("repository-a"), b = fixture("repository-b");
  const original = [inputs(a), inputs(b)];
  const holder = maintenance(1000);
  await started(holder);
  const left = check(a), right = check(b);
  await until(() => left.stderr.includes("waiting") && right.stderr.includes("waiting"), "CLI and maintenance calls did not coordinate");
  await successful(holder);
  await successful(left, true);
  await successful(right, true);

  const active = check(a, "30");
  await ownedScope(active);
  const queued = check(b);
  await until(() => queued.stderr.includes("waiting"), "second CLI did not queue");
  queued.child.kill("SIGTERM");
  assert.equal((await queued.done).code, 143);
  assert.equal(active.child.exitCode, null);
  active.child.kill("SIGINT");
  assert.equal((await active.done).code, 130);
  await successful(check(b), true);

  const cancelled = maintenance(30000);
  await started(cancelled);
  cancelled.child.kill("SIGTERM");
  assert.equal((await cancelled.done).code, 143);
  await successful(maintenance(5));

  for (const launch of [() => maintenance(800), () => check(a, "0.8")]) {
    const orphan = launch();
    if (orphan.child.spawnfile === binary) await ownedScope(orphan);
    else await started(orphan);
    // Allow the inner scope leader to start before terminating only its supervisor.
    await pause(100);
    orphan.child.kill("SIGKILL");
    const waiting = maintenance(5);
    await until(() => waiting.stderr.includes("waiting"), "a surviving scope released the shared resource slot");
    await orphan.done;
    await successful(waiting);
  }
  assert.deepEqual([inputs(a), inputs(b)], original);
  console.log("resource_concurrency: overlapping repositories and runners, queued/active cancellation, supervisor termination, immediate restart and source restoration verified");
}

try { await exercise(); }
finally {
  for (const job of processes) if (job.child.exitCode === null && job.child.signalCode === null) job.child.kill("SIGTERM");
  for (const unit of scopes) {
    // Only identities observed for this test's own processes are cleaned up.
    try { execFileSync("systemctl", ["--user", "stop", unit], { env: environment, stdio: "ignore", timeout: 5000 }); } catch { /* Completed collected scopes may already be absent. */ }
    const identity = unit.slice("hardgate-workload-".length, -".scope".length);
    const directory = path.join(os.tmpdir(), `hardgate-workload-ready-${identity}`);
    fs.rmSync(path.join(directory, "ready"), { force: true });
    try { fs.rmdirSync(directory); } catch (error) { if (error.code !== "ENOENT") throw error; }
  }
  fs.rmSync(root, { recursive: true, force: true });
}
