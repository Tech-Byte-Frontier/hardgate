import { describe, expect, it } from "vitest";
import { App, increment } from "./App";

describe("App", () => {
  it("increments count", () => expect(increment(1)).toBe(2));
  it("exports a component", () => expect(App).toBeDefined());
});
