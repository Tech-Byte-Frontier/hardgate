// The supported distribution contract for new releases. Historical signed tags
// retain their own platform manifests and recovery tooling.
"use strict";

export const NATIVE_PACKAGES = Object.freeze({
  "hardgate-linux-x64": Object.freeze({
    name: "hardgate-linux-x64",
    platform: "linux",
    arch: "x64",
    libc: "glibc",
    target: "x86_64-unknown-linux-gnu",
    archPattern: /x86-64/,
    abi: "gnu",
  }),
  "hardgate-linux-arm64": Object.freeze({
    name: "hardgate-linux-arm64", platform: "linux", arch: "arm64", libc: "glibc",
    target: "aarch64-unknown-linux-gnu", archPattern: /ARM aarch64/, abi: "gnu",
  }),
  "hardgate-darwin-x64": Object.freeze({
    name: "hardgate-darwin-x64", platform: "darwin", arch: "x64", libc: null,
    target: "x86_64-apple-darwin", archPattern: /Mach-O 64-bit.*x86_64/, abi: "darwin",
  }),
  "hardgate-darwin-arm64": Object.freeze({
    name: "hardgate-darwin-arm64", platform: "darwin", arch: "arm64", libc: null,
    target: "aarch64-apple-darwin", archPattern: /Mach-O 64-bit.*arm64/, abi: "darwin",
  }),
});
export const PLATFORM_NAMES = Object.freeze(Object.keys(NATIVE_PACKAGES));
export const PLATFORM_ASSETS = Object.freeze(PLATFORM_NAMES.map((name) => `${name}.tar.gz`));
export const PLATFORM_CONTRACT = Object.freeze(Object.values(NATIVE_PACKAGES).map(({ name, platform, arch, libc }) =>
  Object.freeze({ name, os: [platform], cpu: [arch], libc: libc ? [libc] : undefined })));

export function executableName(packageName) {
  if (!Object.hasOwn(NATIVE_PACKAGES, packageName)) throw new Error(`unsupported native package ${packageName}`);
  return "hardgate";
}
