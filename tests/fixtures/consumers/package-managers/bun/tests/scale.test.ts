import { expect, test } from "bun:test";
import { scale } from "../src/scale";

test("scale doubles", () => expect(scale(2)).toBe(4));
