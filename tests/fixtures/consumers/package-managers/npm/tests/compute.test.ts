import { expect, test } from "jest";
import { compute } from "../src/compute";

test("compute increments", () => expect(compute(1)).toBe(2));
