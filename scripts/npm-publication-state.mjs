// Each invocation publishes at most once, then requires independent byte proof.
"use strict";
import { isRetryableNpmPackError } from "./npm-pack-retry.mjs";
import { registryBackoff, waitForNpmVersion } from "./npm-registry-state.mjs";

export async function publishVerifiedPackage(request, operations) {
  const initial = await waitForNpmVersion(request, false, operations.probe);
  let publication = "existing";
  if (initial.state === "missing") {
    publication = "published";
    try {
      await operations.publish(request);
    } catch (error) {
      publication = "ambiguous";
      // Even a failed response can follow a completed immutable publication.
      // Probe once before deciding whether a permanent failure is final.
      const observed = await waitForNpmVersion(request, false, operations.probe);
      if (observed.state === "missing" && !isRetryableNpmPackError(error)) throw error;
      if (observed.state === "missing") await registryBackoff(request.policy, error);
    }
    await waitForNpmVersion(request, true, operations.probe);
  }
  await operations.verify(request);
  return { publication, identity: `${request.name}@${request.version}`, state: "verified" };
}
