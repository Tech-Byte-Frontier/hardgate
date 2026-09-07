// Apply the newest proof for each native platform from this workflow run.
// Successful matrix jobs may come from earlier attempts during a partial rerun.
import fs from "node:fs";
import path from "node:path";
import { PLATFORM_NAMES } from "./release-platforms.mjs";
import { applyNativeProof } from "./apply-native-receipt.mjs";
import { readReceipt, writeReceiptAtomicSync } from "./release-receipt.mjs";

const [receiptPath, directory, mode] = process.argv.slice(2);
if (!receiptPath || !directory || !["exact", "default"].includes(mode)) {
  throw new Error("usage: apply-portable-proofs.mjs RECEIPT DIRECTORY exact|default");
}
let receipt = readReceipt(receiptPath);
for (const name of PLATFORM_NAMES.filter((name) => name !== "hardgate-linux-x64")) {
  const prefix = `portable-${mode}-${name}-attempt-`;
  const candidates = fs.readdirSync(directory)
    .filter((entry) => entry.startsWith(prefix) && /^[1-9][0-9]*$/.test(entry.slice(prefix.length)))
    .sort((a, b) => Number(b.slice(prefix.length)) - Number(a.slice(prefix.length)));
  if (!candidates.length) throw new Error(`missing ${mode} native consumer proof for ${name}`);
  const proof = JSON.parse(fs.readFileSync(path.join(directory, candidates[0], `${name}.json`), "utf8"));
  if (proof.package !== name || proof.mode !== mode) throw new Error(`wrong consumer proof for ${name}`);
  receipt = applyNativeProof(receipt, proof);
}
writeReceiptAtomicSync(receiptPath, receipt);
