import { expect, test } from "@playwright/test";
import { demoFindings, demoChains, demoHealth, DEMO_ADDRESS } from "../src/lib/demo";

// Live API responses are mocked at the browser boundary: no database or provider calls.
test("demo filtering, pagination, investigation and export", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await expect(page.getByText("You’re exploring a demo.")).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(6);
  await expect(page.locator(".highcharts-root")).toBeVisible();
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(page.getByText("7–12 of 48 findings")).toBeVisible();
  await page.getByRole("combobox", { name: "Filter by severity" }).selectOption("critical");
  await expect(page.getByText("1–6 of 8 findings")).toBeVisible();
  await page.locator(".finding-title").first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Incident investigation" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Revision history" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export findings" }).click();
  expect((await download).suggestedFilename()).toBe("chain-siren-demo-findings.json");
  await page.getByRole("textbox", { name: "Search findings" }).fill("does-not-exist");
  await expect(page.getByText("No findings match your filters")).toBeVisible();
  expect(errors).toEqual([]);
});

test("mobile navigation and address lookup remain usable", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByRole("button", { name: "Address explorer", exact: true }).click();
  await page.getByRole("button", { name: "Try the sample address" }).click();
  await expect(page.getByRole("heading", { name: "Watch profile" })).toBeVisible();
  await expect(page.locator(".profile-panel").getByText(DEMO_ADDRESS, { exact: true })).toBeVisible();
  const width = await page.evaluate(() => ({ body: document.documentElement.scrollWidth, screen: window.innerWidth }));
  expect(width.body).toBeLessThanOrEqual(width.screen);
  await page.getByRole("button", { name: "Networks", exact: true }).click();
  await expect(page.locator(".chain-card")).toHaveCount(4);
});

test("connects with an in-memory key and clears live data on disconnect", async ({ page }) => {
  const findings = demoFindings(Date.now()).slice(0, 1).map((finding) => ({ ...finding, finding_type: "live_test_signal" }));
  await page.route("**/api/monitor/**", async (route) => {
    expect(route.request().headers().authorization).toBe("Bearer cs_test_only");
    const path = new URL(route.request().url()).pathname;
    const body = path.endsWith("/chains") ? { chains: demoChains(Date.now()) }
      : path.endsWith("/ready") ? demoHealth : { incidents: findings };
    await route.fulfill({ json: body });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Connect live", exact: true }).click();
  await page.getByLabel("API key", { exact: true }).fill("cs_test_only");
  await page.getByRole("button", { name: "Connect to live data" }).click();
  await expect(page.getByText("Live monitor", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "live test signal", exact: true })).toBeVisible();
  expect(await page.evaluate(() => JSON.stringify({ local: { ...localStorage }, session: { ...sessionStorage } }))).not.toContain("cs_test_only");
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(page.getByText("You’re exploring a demo.")).toBeVisible();
  await expect(page.getByRole("button", { name: "live test signal", exact: true })).toHaveCount(0);
});

test("invalid credentials show an error and never enter live mode", async ({ page }) => {
  await page.route("**/api/monitor/**", (route) => route.fulfill({ status: 401, json: { error: "API key rejected or expired." } }));
  await page.goto("/");
  await page.getByRole("button", { name: "Connect live", exact: true }).click();
  await page.getByLabel("API key", { exact: true }).fill("invalid");
  await page.getByRole("button", { name: "Connect to live data" }).click();
  await expect(page.getByRole("alert")).toContainText("API key rejected or expired.");
  await expect(page.getByText("Live monitor", { exact: true })).toHaveCount(0);
});

test("proxy rejects missing credentials and unsupported paths without forwarding", async ({ request }) => {
  expect((await request.get("/api/monitor/v1/incidents")).status()).toBe(401);
  expect((await request.get("/api/monitor/v1/admin", { headers: { Authorization: "Bearer test" } })).status()).toBe(404);
  expect((await request.get("/api/monitor/v1/incidents?url=https://example.com", { headers: { Authorization: "Bearer test" } })).status()).toBe(400);
  expect((await request.post("/api/monitor/v1/incidents")).status()).toBe(405);
});
