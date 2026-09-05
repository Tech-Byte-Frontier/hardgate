// A deliberately narrow npm registry for exact local package archives.
"use strict";

import crypto from "node:crypto";
import http from "node:http";

const MAX_REGISTRY_REQUESTS = 10_000;

function sendResponse(response, status, body, contentType = "text/plain; charset=utf-8") {
  const bytes = Buffer.isBuffer(body) ? body : Buffer.from(body);
  response.writeHead(status, {
    "content-type": contentType,
    "content-length": bytes.length,
    "cache-control": "no-store",
  });
  if (response.req.method !== "HEAD") response.end(bytes);
  else response.end();
}

function artifactKey(name) {
  return crypto.createHash("sha256").update(name).digest("hex");
}

function registryManifest(artifact, baseUrl) {
  const manifest = structuredClone(artifact.manifest);
  manifest._id = `${artifact.name}@${artifact.version}`;
  manifest.dist = {
    ...(manifest.dist ?? {}),
    shasum: artifact.shasum,
    integrity: artifact.integrity,
    tarball: `${baseUrl}__hardgate-tarball/${artifactKey(artifact.name)}.tgz`,
  };
  return manifest;
}

function packageMetadata(artifact, baseUrl) {
  return JSON.stringify({
    _id: artifact.name,
    name: artifact.name,
    "dist-tags": { latest: artifact.version },
    versions: { [artifact.version]: registryManifest(artifact, baseUrl) },
    time: {
      created: new Date(0).toISOString(),
      modified: new Date(0).toISOString(),
      [artifact.version]: new Date(0).toISOString(),
    },
  });
}

function recordRequest(requests, request, pathname, status) {
  if (requests.length >= MAX_REGISTRY_REQUESTS) return;
  requests.push({ method: request.method, path: pathname, status });
}

function packageForPath(artifacts, pathname) {
  return [...artifacts.values()].find((artifact) => pathname === `/${artifact.name}` || pathname === `/${artifact.name}/${artifact.version}`);
}

function tarballForPath(artifacts, pathname) {
  if (!pathname.startsWith("/__hardgate-tarball/")) return null;
  const key = pathname.slice("/__hardgate-tarball/".length).replace(/\.tgz$/, "");
  return [...artifacts.values()].find((artifact) => artifactKey(artifact.name) === key) ?? false;
}

function requestHandler({ artifacts, requests, server, request, response }) {
  let pathname;
  try {
    pathname = decodeURIComponent(new URL(request.url, "http://127.0.0.1").pathname);
  } catch {
    recordRequest(requests, request, request.url, 400);
    sendResponse(response, 400, "invalid request path\n");
    return;
  }
  if (request.method !== "GET" && request.method !== "HEAD") {
    recordRequest(requests, request, pathname, 405);
    sendResponse(response, 405, "method not allowed\n");
    return;
  }
  const tarball = tarballForPath(artifacts, pathname);
  if (tarball === false) {
    recordRequest(requests, request, pathname, 404);
    sendResponse(response, 404, "not found\n");
    return;
  }
  if (tarball) {
    recordRequest(requests, request, pathname, 200);
    sendResponse(response, 200, tarball.archiveBytes, "application/octet-stream");
    return;
  }
  const artifact = packageForPath(artifacts, pathname);
  if (!artifact) {
    recordRequest(requests, request, pathname, 404);
    sendResponse(response, 404, "not found\n");
    return;
  }
  const address = server.address();
  const baseUrl = `http://127.0.0.1:${typeof address === "object" && address ? address.port : 0}/`;
  recordRequest(requests, request, pathname, 200);
  const exactVersion = pathname === `/${artifact.name}/${artifact.version}`;
  const body = exactVersion ? JSON.stringify(registryManifest(artifact, baseUrl)) : packageMetadata(artifact, baseUrl);
  sendResponse(response, 200, body, "application/json; charset=utf-8");
}

export async function startLocalRegistry(artifacts) {
  const requests = [];
  const server = http.createServer((request, response) => requestHandler({ artifacts, requests, server, request, response }));
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("localhost registry did not expose a TCP port");
  return {
    baseUrl: `http://127.0.0.1:${address.port}/`,
    requests,
    async close() {
      server.closeAllConnections?.();
      await new Promise((resolve) => server.close(() => resolve()));
    },
  };
}
