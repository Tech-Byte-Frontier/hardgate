import { expect, test } from "vitest";
import { pageTitle } from "./page";

test("page title includes the name", () => expect(pageTitle("fixture")).toBe("Next:fixture"));
