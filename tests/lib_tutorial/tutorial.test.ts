/*
 * Tests for tutorial provider script.
 * Runs tutorial.ts via subprocess with various paths and validates JSON output.
 */

import { describe, test, expect } from "bun:test";
import { resolve } from "path";

const scriptPath = resolve(__dirname, "../../lib/lib_tutorial/tutorial.ts");

async function runTutorial(path: string): Promise<unknown[]> {
  const proc = Bun.spawn(["bun", "run", scriptPath, path], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const text = await new Response(proc.stdout).text();
  await proc.exited;
  return JSON.parse(text);
}

describe("tutorial provider", () => {
  test("root path returns 7 sections", async () => {
    const result = await runTutorial("/");
    expect(result).toBeArray();
    expect(result.length).toBe(7);
  });

  test("root sections have correct names", async () => {
    const result = await runTutorial("/");
    const keys = result.map((item) => {
      if (typeof item === "string") return item;
      return Object.keys(item as Record<string, unknown>)[0];
    });
    expect(keys).toContain("Welcome");
    expect(keys).toContain("Navigation");
    expect(keys).toContain("Editing");
    expect(keys).toContain("Commands");
    expect(keys).toContain("File Browser");
    expect(keys).toContain("Links");
    expect(keys).toContain("Next Steps");
  });

  test("/Welcome returns children with checkbox and radio elements", async () => {
    const result = await runTutorial("/Welcome");
    expect(result).toBeArray();
    expect(result.length).toBeGreaterThan(0);

    // Should contain checkbox items
    const strings = result.filter((item) => typeof item === "string") as string[];
    const hasCheckbox = strings.some(
      (s) => s.includes("<checkbox")
    );
    expect(hasCheckbox).toBe(true);

    // Should contain radio parent object
    const objects = result.filter((item) => typeof item === "object" && item !== null);
    const hasRadio = objects.some((obj) => {
      const key = Object.keys(obj as Record<string, unknown>)[0];
      return key.includes("<radio>");
    });
    expect(hasRadio).toBe(true);
  });

  test("/Navigation returns 2 subsections", async () => {
    const result = await runTutorial("/Navigation");
    expect(result).toBeArray();
    expect(result.length).toBe(2);

    const keys = result.map((item) => {
      if (typeof item === "string") return item;
      return Object.keys(item as Record<string, unknown>)[0];
    });
    expect(keys).toContain("Moving Around");
    expect(keys).toContain("Modes");
  });

  test("/Navigation/Modes returns mode descriptions", async () => {
    const result = await runTutorial("/Navigation/Modes");
    expect(result).toBeArray();
    expect(result.length).toBe(4);
    // All items should be strings
    for (const item of result) {
      expect(typeof item).toBe("string");
    }
  });

  test("/Editing returns editing instructions", async () => {
    const result = await runTutorial("/Editing");
    expect(result).toBeArray();
    expect(result.length).toBe(4);
  });

  test("invalid path returns empty array", async () => {
    const result = await runTutorial("/NonExistentSection");
    expect(result).toBeArray();
    expect(result.length).toBe(0);
  });

  test("/Links contains a link element", async () => {
    const result = await runTutorial("/Links");
    expect(result).toBeArray();

    const objects = result.filter((item) => typeof item === "object" && item !== null);
    const hasLink = objects.some((obj) => {
      const key = Object.keys(obj as Record<string, unknown>)[0];
      return key.includes("<link>");
    });
    expect(hasLink).toBe(true);
  });

  test("/Welcome contains image elements", async () => {
    const result = await runTutorial("/Welcome");
    const strings = result.filter((item) => typeof item === "string") as string[];
    const hasImage = strings.some((s) => s.includes("<image>"));
    expect(hasImage).toBe(true);
  });
});
