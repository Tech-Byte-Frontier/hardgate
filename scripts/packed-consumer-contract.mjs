// Package-manifest and platform-contract validation for packed consumers.
"use strict";

const LIFECYCLE_HOOKS = [
  "preinstall", "install", "postinstall", "prepare", "prepublish", "prepublishOnly", "prepack", "postpack",
];

function fail(message) {
  throw new Error(message);
}

function dependencyEntries(manifest, field) {
  const value = manifest[field];
  if (value === undefined) return [];
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${manifest.name} ${field} must be an object`);
  return Object.entries(value);
}

function validateDependencyFields(manifest) {
  for (const field of ["dependencies", "devDependencies", "peerDependencies"]) {
    if (Object.hasOwn(manifest, field)) fail(`${manifest.name} must not declare ${field}`);
  }
  if (Object.hasOwn(manifest, "bundledDependencies") || Object.hasOwn(manifest, "bundleDependencies")) {
    fail(`${manifest.name} must not declare bundled dependencies`);
  }
}

function validateLifecycleScripts(manifest) {
  const scripts = manifest.scripts;
  if (scripts !== undefined && (!scripts || typeof scripts !== "object" || Array.isArray(scripts))) fail(`${manifest.name} scripts must be an object`);
  for (const hook of LIFECYCLE_HOOKS) {
    if (Object.hasOwn(scripts ?? {}, hook)) fail(`${manifest.name} must not declare npm lifecycle hook ${hook}`);
  }
}

function validateOptionalDependencies(manifest) {
  for (const [name, value] of dependencyEntries(manifest, "optionalDependencies")) {
    if (typeof value !== "string" || /^(?:file|link|workspace|npm|http|https|git|github|ssh):/i.test(value)) {
      fail(`${manifest.name} optionalDependency ${name} must be a registry version`);
    }
  }
}

export function validateManifestContract(manifest) {
  validateDependencyFields(manifest);
  validateLifecycleScripts(manifest);
  validateOptionalDependencies(manifest);
}

function exactManifestArray(manifest, field, expected, packageName) {
  const actual = manifest[field];
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(`${packageName} manifest ${field}=${JSON.stringify(actual)} expected ${JSON.stringify(expected)}`);
  }
}

export function validatePlatformArtifact({ artifact, descriptor, expectedVersion }) {
  if (!artifact) fail(`--packages-dir is missing ${descriptor.name}@${expectedVersion}`);
  const nativeBytes = artifact.entries.get("package/bin/hardgate");
  if (!nativeBytes) fail(`${descriptor.name}@${expectedVersion}.tgz is missing package/bin/hardgate`);
  if (nativeBytes.length === 0) fail(`${descriptor.name} package/bin/hardgate is empty`);
  const nativeMode = artifact.entryModes.get("package/bin/hardgate");
  if ((nativeMode & 0o111) === 0) fail(`${descriptor.name} package/bin/hardgate is not executable in the archive`);
  exactManifestArray(artifact.manifest, "os", descriptor.os, descriptor.name);
  exactManifestArray(artifact.manifest, "cpu", descriptor.cpu, descriptor.name);
  exactManifestArray(artifact.manifest, "libc", descriptor.libc, descriptor.name);
  if (Object.hasOwn(artifact.manifest, "optionalDependencies")) fail(`${descriptor.name} must not declare optionalDependencies`);
  validateManifestContract(artifact.manifest);
  return nativeBytes;
}

function validateWrapperBin(manifest, wrapperName) {
  const bin = manifest.bin;
  if (!bin || typeof bin !== "object" || Array.isArray(bin) || Object.keys(bin).length !== 1 || bin.hardgate !== "bin/hardgate.js") {
    fail(`${wrapperName} manifest bin.hardgate must be exactly bin/hardgate.js`);
  }
}

function validateWrapperOptionalDependencies(manifest, expectedVersion, wrapperName, platformPackages) {
  const optional = manifest.optionalDependencies ?? {};
  const expectedNames = [...platformPackages].sort();
  if (JSON.stringify(Object.keys(optional).sort()) !== JSON.stringify(expectedNames)) {
    fail(`${wrapperName} optionalDependencies do not match the supported Linux x64 GNU platform package`);
  }
  for (const name of platformPackages) {
    if (optional[name] !== expectedVersion) fail(`${wrapperName} optionalDependencies[${name}] must be ${expectedVersion}`);
  }
}

export function validateWrapper(wrapper, expectedVersion, wrapperName, platformPackages) {
  if (!wrapper) fail(`--packages-dir is missing ${wrapperName}@${expectedVersion}.tgz`);
  const launcherBytes = wrapper.entries.get("package/bin/hardgate.js");
  if (!launcherBytes) fail(`${wrapperName}@${expectedVersion}.tgz is missing package/bin/hardgate.js`);
  for (const [field, expected] of Object.entries({ os: ["linux"], cpu: ["x64"], libc: ["glibc"] })) {
    exactManifestArray(wrapper.manifest, field, expected, wrapperName);
  }
  validateWrapperBin(wrapper.manifest, wrapperName);
  validateWrapperOptionalDependencies(wrapper.manifest, expectedVersion, wrapperName, platformPackages);
  return launcherBytes;
}

export function validateHost({ hostArtifact, host, descriptor, expectedBytes, expectedHash, expectedVersion }) {
  if (!hostArtifact) fail(`--packages-dir is missing host optional dependency ${host}@${expectedVersion}`);
  const nativeBytes = validatePlatformArtifact({ artifact: hostArtifact, descriptor, expectedVersion });
  if (!nativeBytes.equals(expectedBytes)) fail(`${host} archive binary bytes do not match --binary (expected sha256 ${expectedHash})`);
}
