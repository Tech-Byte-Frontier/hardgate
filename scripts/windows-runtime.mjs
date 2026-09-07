// Check the PE import table without requiring Visual Studio inspection tools.
import assert from "node:assert/strict";

export function windowsImports(bytes) {
  assert.equal(bytes.toString("ascii", 0, 2), "MZ", "expected a Windows executable");
  const header = bytes.readUInt32LE(0x3c);
  assert.equal(bytes.toString("ascii", header, header + 4), "PE\0\0", "invalid PE signature");
  const optional = header + 24;
  assert.equal(bytes.readUInt16LE(optional), 0x20b, "expected PE32+");
  const sectionStart = optional + bytes.readUInt16LE(header + 20);
  const sections = [];
  for (let index = 0; index < bytes.readUInt16LE(header + 6); index += 1) {
    const entry = sectionStart + index * 40;
    sections.push({ start: bytes.readUInt32LE(entry + 12), size: bytes.readUInt32LE(entry + 16), offset: bytes.readUInt32LE(entry + 20) });
  }
  const offset = (address) => {
    const section = sections.find(({ start, size }) => address >= start && address < start + size);
    assert.ok(section, "PE import address must resolve inside a section");
    return section.offset + address - section.start;
  };
  let entry = offset(bytes.readUInt32LE(optional + 120));
  const imports = [];
  while (entry + 20 <= bytes.length && bytes.subarray(entry, entry + 20).some((value) => value !== 0)) {
    const name = offset(bytes.readUInt32LE(entry + 12));
    const end = bytes.indexOf(0, name);
    assert.ok(end > name && end - name < 256, "invalid PE import name");
    imports.push(bytes.toString("ascii", name, end));
    entry += 20;
  }
  assert.ok(imports.length > 0 && entry + 20 <= bytes.length, "PE imports must terminate");
  return imports;
}

export function verifyWindowsRuntime(bytes) {
  const imports = windowsImports(bytes);
  assert.deepEqual(imports.filter((name) => /^(?:vcruntime|msvcp|concrt)\d.*\.dll$/i.test(name)), [],
    "native installation must not require the Visual C++ redistributable");
  return imports;
}
