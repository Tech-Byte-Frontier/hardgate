import { expect, test } from "@jest/globals";
import { sum } from "../../src/sum";

test("sum adds values", () => expect(sum(2, 3)).toBe(5));
