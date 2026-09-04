import { expect, test } from "vitest";
import { inspect } from "../src/inspect";

test("inspect trims", () => expect(inspect(" value ")).toBe("value"));
