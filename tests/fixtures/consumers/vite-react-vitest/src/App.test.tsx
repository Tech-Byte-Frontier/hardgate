import { describe, expect, it } from "vitest";
import { App, counterReducer, increment } from "./App";

describe("App", () => {
  it("increments count", () => expect(increment(1)).toBe(2));
  it("reduces counter actions", () => {
    expect(counterReducer(1, "increment")).toBe(2);
    expect(counterReducer(4, "reset")).toBe(0);
  });
  it("exports a component", () => expect(App).toBeDefined());
});
