import { expect, test } from "jest";
import { format } from "../src/format";

test("format uppercases", () => expect(format("ok")).toBe("OK"));
