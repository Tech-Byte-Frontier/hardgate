// A deliberately narrow npm registry for exact local package archives.
"use strict";

import crypto from "node:crypto";
import http from "node:http";

const DEFAULT_MAX_REGISTRY_REQUESTS = 10_000;
const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;

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

function requestPath(request) {
  try {
    return decodeURIComponent(new URL(request.url, "http://127.0.0.1").pathname);
  } catch {
    return null;
  }
}

function respondInvalidPath({ request, response, requests }) {
  recordRequest(requests, request, request.url, 400);
  sendResponse(response, 400, "invalid request path\n");
}

function respondMethodNotAllowed({ request, response, requests, pathname }) {
  recordRequest(requests, request, pathname, 405);
  sendResponse(response, 405, "method not allowed\n");
}

function respondArtifact({ request, response, requests, pathname, artifact, server, tarball = false }) {
  recordRequest(requests, request, pathname, 200);
  if (tarball) {
    sendResponse(response, 200, artifact.archiveBytes, "application/octet-stream");
    return;
  }
  const address = server.address();
  const baseUrl = `http://127.0.0.1:${typeof address === "object" && address ? address.port : 0}/`;
  const exactVersion = pathname === `/${artifact.name}/${artifact.version}`;
  const body = exactVersion ? JSON.stringify(registryManifest(artifact, baseUrl)) : packageMetadata(artifact, baseUrl);
  sendResponse(response, 200, body, "application/json; charset=utf-8");
}

function respondPath(context) {
  const { request, response, requests, artifacts, server, pathname } = context;
  if (request.method !== "GET" && request.method !== "HEAD") return respondMethodNotAllowed(context);
  const tarball = tarballForPath(artifacts, pathname);
  if (tarball === false) {
    recordRequest(requests, request, pathname, 404);
    return sendResponse(response, 404, "not found\n");
  }
  if (tarball) return respondArtifact({ ...context, artifact: tarball, tarball: true });
  const artifact = packageForPath(artifacts, pathname);
  if (!artifact) {
    recordRequest(requests, request, pathname, 404);
    return sendResponse(response, 404, "not found\n");
  }
  return respondArtifact({ ...context, artifact, server });
}

function requestHandler(context) {
  const { requests, maxRequests, request, response, requestTimeoutMs } = context;
  if (requests.length >= maxRequests) {
    request.resume();
    return sendResponse(response, 429, "registry request limit exceeded\n");
  }
  request.setTimeout(requestTimeoutMs, () => request.destroy());
  const pathname = requestPath(request);
  if (pathname === null) return respondInvalidPath(context);
  return respondPath({ ...context, pathname });
}

export async function startLocalRegistry(artifacts, { maxRequests = DEFAULT_MAX_REGISTRY_REQUESTS, requestTimeoutMs = DEFAULT_REQUEST_TIMEOUT_MS } = {}) {
  if (!Number.isSafeInteger(maxRequests) || maxRequests < 1) throw new Error("registry maxRequests must be a positive integer");
  if (!Number.isSafeInteger(requestTimeoutMs) || requestTimeoutMs < 1) throw new Error("registry requestTimeoutMs must be a positive integer");
  const requests = [];
  const server = http.createServer((request, response) => requestHandler({ artifacts, requests, server, request, response, maxRequests, requestTimeoutMs }));
  server.headersTimeout = requestTimeoutMs;
  server.requestTimeout = requestTimeoutMs;
  server.timeout = requestTimeoutMs;
  server.keepAliveTimeout = 1_000;
  server.maxHeadersCount = 64;
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
      server.closeIdleConnections?.();
      await new Promise((resolve) => server.close(() => resolve()));
    },
  };
}
