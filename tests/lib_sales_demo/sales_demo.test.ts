/*
 * Tests for sales demo provider script.
 * Runs sales_demo.ts via subprocess with various paths and validates JSON output.
 */

import { describe, test, expect } from "bun:test";
import { resolve } from "path";

const scriptPath = resolve(__dirname, "../../lib/lib_sales_demo/sales_demo.ts");

async function runSalesDemo(path: string): Promise<unknown> {
  const proc = Bun.spawn(["bun", "run", scriptPath, path], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const text = await new Response(proc.stdout).text();
  await proc.exited;
  return JSON.parse(text);
}

function getChildren(result: unknown): unknown[] {
  if (Array.isArray(result)) return result;
  return (result as Record<string, unknown>).children as unknown[];
}

describe("sales demo provider", () => {
  test("root path returns object with children and dashboardImage", async () => {
    const result = await runSalesDemo("/") as Record<string, unknown>;
    expect(result).toBeObject();
    expect(result.children).toBeArray();
    expect((result.children as unknown[]).length).toBeGreaterThan(0);
    expect(typeof result.dashboardImage).toBe("string");
    expect((result.dashboardImage as string).endsWith(".webp")).toBe(true);
  });

  test("root entries include mandatory items as direct children", async () => {
    const result = getChildren(await runSalesDemo("/"));
    // Should have at least one non-"Add element:" item
    const nonAddItems = result.filter((item) => {
      if (typeof item === "string") return true;
      const key = Object.keys(item as Record<string, unknown>)[0];
      return key !== "Add element:";
    });
    expect(nonAddItems.length).toBeGreaterThan(0);
  });

  test("root may contain Add element section with button tags", async () => {
    const result = getChildren(await runSalesDemo("/"));

    // Look for "Add element:" section
    const addSection = result.find((item) => {
      if (typeof item === "string") return false;
      const key = Object.keys(item as Record<string, unknown>)[0];
      return key === "Add element:";
    });

    if (addSection) {
      const key = Object.keys(addSection as Record<string, unknown>)[0];
      const children = (addSection as Record<string, unknown[]>)[key];
      expect(children).toBeArray();
      // Each child should contain button tags
      for (const child of children) {
        expect(typeof child).toBe("string");
        expect((child as string).includes("<button>")).toBe(true);
        expect((child as string).includes("</button>")).toBe(true);
      }
    }
  });

  test("invalid path returns empty array", async () => {
    const result = await runSalesDemo("/CompletelyNonExistentPath");
    expect(result).toBeArray();
    expect(result as unknown[]).toHaveLength(0);
  });

  test("sub-path output has children array", async () => {
    const root = getChildren(await runSalesDemo("/"));
    const firstObj = root.find((item) => {
      if (typeof item === "string") return false;
      const key = Object.keys(item as Record<string, unknown>)[0];
      return key !== "Add element:";
    });

    if (firstObj && typeof firstObj === "object") {
      const key = Object.keys(firstObj as Record<string, unknown>)[0];
      const result = await runSalesDemo("/" + key);
      const children = getChildren(result);
      expect(children).toBeArray();
    }
  });

  test("navigating into a top-level entry returns its children", async () => {
    // Get the first object entry from root
    const root = getChildren(await runSalesDemo("/"));
    const firstObj = root.find((item) => {
      if (typeof item === "string") return false;
      const key = Object.keys(item as Record<string, unknown>)[0];
      return key !== "Add element:";
    });

    if (firstObj && typeof firstObj === "object") {
      const key = Object.keys(firstObj as Record<string, unknown>)[0];
      const children = await runSalesDemo("/" + key);
      expect(children).toBeArray();
    }
  });
});
