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
});
export const PLATFORM_NAMES = Object.freeze(Object.keys(NATIVE_PACKAGES));
export const PLATFORM_ASSETS = Object.freeze(PLATFORM_NAMES.map((name) => `${name}.tar.gz`));
export const PLATFORM_CONTRACT = Object.freeze(Object.values(NATIVE_PACKAGES).map(({ name, platform, arch, libc }) =>
  Object.freeze({ name, os: [platform], cpu: [arch], libc: [libc] })));
