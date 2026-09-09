import fs from "node:fs";
import path from "node:path";

// The Rust supervisor uses the same flock file. Keep mutation's separate lease.
export function openWorkloadSlot(directory = `/tmp/hardgate-workload-${process.getuid()}`) {
  try { fs.mkdirSync(directory, { mode: 0o700 }); }
  catch (error) { if (error.code !== "EEXIST") throw error; }
  validateDirectory(directory);
  const flags = fs.constants.O_RDONLY | fs.constants.O_CREAT | fs.constants.O_NOFOLLOW;
  const fd = fs.openSync(path.join(directory, "slot.lock"), flags, 0o600);
  try {
    const file = fs.fstatSync(fd);
    if (!file.isFile() || file.uid !== process.getuid() || file.nlink !== 1 || (file.mode & 0o7777) !== 0o600) {
      throw new Error("workload slot must be an owner-controlled regular file with mode 0600 and no hard links");
    }
    return fd;
  } catch (error) { fs.closeSync(fd); throw error; }
}

function validateDirectory(directory) {
  const parent = fs.lstatSync(directory);
  if (!parent.isDirectory() || parent.uid !== process.getuid() || (parent.mode & 0o7777) !== 0o700) {
    throw new Error("workload slot directory must be a private, owner-controlled directory");
  }
}

export async function acquireWorkloadSlot(fd, execute) {
  const stdio = ["ignore", "inherit", "inherit", fd];
  const immediate = await execute("flock", ["--exclusive", "--nonblock", "--conflict-exit-code", "75", "3"], { stdio });
  if (immediate === 0) return;
  if (immediate !== 75) throw new Error("could not acquire the per-user workload slot");
  console.error("hardgate: another workload owns the per-user resource slot; waiting (up to 30 minutes, Ctrl-C to cancel)");
  const queued = performance.now();
  const waited = await execute("flock", ["--exclusive", "--timeout", "1800", "--conflict-exit-code", "75", "3"], { stdio });
  if (waited === 75) throw new Error("workload contention: the per-user resource slot remained busy for 30 minutes; no evaluation was started");
  if (waited !== 0) throw new Error("could not wait for the per-user workload slot");
  console.error(`hardgate: workload slot acquired after ${((performance.now() - queued) / 1000).toFixed(1)}s queued; starting execution`);
}
