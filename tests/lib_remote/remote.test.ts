/*
 * Tests for remote FFON provider script.
 * Tests wrapWithLinks logic and end-to-end remote fetching via a mock HTTP server.
 */

import { describe, test, expect, afterAll, beforeAll } from "bun:test";
import { resolve } from "path";
import { mkdtempSync, writeFileSync, mkdirSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

const scriptPath = resolve(__dirname, "../../lib/lib_remote/remote.ts");

// Create a temp HOME so remote.ts reads settings from $HOME/.config/sicompass/
const tmpHome = mkdtempSync(join(tmpdir(), "sicompass_remote_test_"));
const configDir = join(tmpHome, ".config", "sicompass");
mkdirSync(configDir, { recursive: true });

function writeSettings(settings: Record<string, unknown>) {
  writeFileSync(join(configDir, "settings.json"), JSON.stringify(settings));
}

async function runRemote(
  providerName: string,
  serverPort: number,
): Promise<unknown[]> {
  const proc = Bun.spawn(["bun", "run", scriptPath, providerName], {
    stdout: "pipe",
    stderr: "pipe",
    env: { ...process.env, HOME: tmpHome },
  });
  const text = await new Response(proc.stdout).text();
  await proc.exited;
  return JSON.parse(text);
}

describe("remote provider", () => {
  test("no remote URL configured returns error message", async () => {
    writeSettings({});
    const result = await runRemote("myservice", 0);
    expect(result).toBeArray();
    expect(result.length).toBe(1);
    expect(result[0]).toContain("No remote URL configured");
  });

  test("successful fetch wraps objects with link tags", async () => {
    const server = Bun.serve({
      port: 0,
      fetch() {
        return new Response(
          JSON.stringify([{ Products: ["item1"] }, "plain string"]),
          { headers: { "Content-Type": "application/json" } },
        );
      },
    });
    writeSettings({
      myservice: { remoteUrl: `http://localhost:${server.port}` },
    });
    const result = await runRemote("myservice", server.port);
    server.stop();

    expect(result).toBeArray();
    expect(result.length).toBe(2);

    // First element: object should be wrapped with <link> tag
    const first = result[0] as Record<string, unknown>;
    const key = Object.keys(first)[0];
    expect(key).toContain("<link>");
    expect(key).toContain("Products");

    // Second element: string should pass through unchanged
    expect(result[1]).toBe("plain string");
  });

  test("does not double-wrap existing link tags", async () => {
    const server = Bun.serve({
      port: 0,
      fetch() {
        return new Response(
          JSON.stringify([
            { "<link>http://existing</link>Already Linked": [] },
          ]),
          { headers: { "Content-Type": "application/json" } },
        );
      },
    });
    writeSettings({
      myservice: { remoteUrl: `http://localhost:${server.port}` },
    });
    const result = await runRemote("myservice", server.port);
    server.stop();

    expect(result).toBeArray();
    const first = result[0] as Record<string, unknown>;
    const key = Object.keys(first)[0];
    const linkCount = (key.match(/<link>/g) || []).length;
    expect(linkCount).toBe(1);
  });

  test("server error returns error message", async () => {
    const server = Bun.serve({
      port: 0,
      fetch() {
        return new Response("Internal Server Error", { status: 500 });
      },
    });
    writeSettings({
      myservice: { remoteUrl: `http://localhost:${server.port}` },
    });
    const result = await runRemote("myservice", server.port);
    server.stop();

    expect(result).toBeArray();
    expect(result.length).toBe(1);
    expect(result[0]).toContain("Failed to fetch");
    expect(result[0]).toContain("500");
  });

  test("server returns non-array returns error", async () => {
    const server = Bun.serve({
      port: 0,
      fetch() {
        return new Response(JSON.stringify({ not: "array" }), {
          headers: { "Content-Type": "application/json" },
        });
      },
    });
    writeSettings({
      myservice: { remoteUrl: `http://localhost:${server.port}` },
    });
    const result = await runRemote("myservice", server.port);
    server.stop();

    expect(result).toBeArray();
    expect(result.length).toBe(1);
    expect(result[0]).toContain("Invalid response");
  });

  test("wraps with correct link URL encoding", async () => {
    const server = Bun.serve({
      port: 0,
      fetch() {
        return new Response(JSON.stringify([{ "My Folder": [] }]), {
          headers: { "Content-Type": "application/json" },
        });
      },
    });
    const baseUrl = `http://localhost:${server.port}`;
    writeSettings({
      myservice: { remoteUrl: baseUrl },
    });
    const result = await runRemote("myservice", server.port);
    server.stop();

    const first = result[0] as Record<string, unknown>;
    const key = Object.keys(first)[0];
    expect(key).toContain(encodeURIComponent("My Folder"));
    expect(key).toContain("My Folder");
  });
});
