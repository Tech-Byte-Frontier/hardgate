import { expect, test } from "@playwright/test";
import { homeTitle } from "../src/home";

test("home title", async () => expect(homeTitle("fixture")).toContain("Home"));
