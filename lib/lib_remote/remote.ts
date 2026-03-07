// Remote FFON provider script
// Run with: bun run remote.ts <providerName>
// On first fetch, reads settings.json for remoteUrl, fetches root FFON from server,
// and wraps sub-objects with <link> tags for lazy navigation.

import { readFileSync } from "fs";
import { homedir } from "os";
import { join } from "path";

const providerName = process.argv[2] || "";

interface Settings {
  [section: string]: {
    remoteUrl?: string;
    apiKey?: string;
    [key: string]: string | undefined;
  };
}

function loadSettings(): Settings {
  const configPath = join(homedir(), ".config", "sicompass", "settings.json");
  try {
    return JSON.parse(readFileSync(configPath, "utf-8"));
  } catch {
    return {};
  }
}

type FfonElement = string | { [key: string]: FfonElement[] };

// Wrap top-level objects with <link> tags pointing to the remote server
function wrapWithLinks(
  elements: FfonElement[],
  baseUrl: string,
): FfonElement[] {
  return elements.map((elem) => {
    if (typeof elem === "string") return elem;
    // Object: wrap key with <link> tag
    const keys = Object.keys(elem);
    if (keys.length !== 1) return elem;
    const key = keys[0];
    // If it already has a <link> tag, leave it
    if (key.includes("<link>")) return elem;
    // Wrap: the display name is the key, the link URL is baseUrl + encoded key
    const linkUrl = `${baseUrl}/${encodeURIComponent(key)}`;
    const newKey = `<link>${linkUrl}</link>${key}`;
    return { [newKey]: [] };
  });
}

async function main() {
  const settings = loadSettings();
  const section = settings[providerName];

  if (!section?.remoteUrl) {
    console.log(
      JSON.stringify([`No remote URL configured for "${providerName}"`]),
    );
    process.exit(0);
  }

  const { remoteUrl, apiKey } = section;
  const rootUrl = `${remoteUrl}/root`;

  try {
    const headers: Record<string, string> = {
      Accept: "application/json",
    };
    if (apiKey) {
      headers["Authorization"] = `Bearer ${apiKey}`;
    }

    const response = await fetch(rootUrl, { headers });
    if (!response.ok) {
      console.log(
        JSON.stringify([
          `Failed to fetch from ${remoteUrl}: ${response.status} ${response.statusText}`,
        ]),
      );
      process.exit(0);
    }

    const data = (await response.json()) as FfonElement[];
    if (!Array.isArray(data)) {
      console.log(JSON.stringify([`Invalid response from ${remoteUrl}`]));
      process.exit(0);
    }

    // Wrap objects with <link> tags for lazy sub-navigation
    const wrapped = wrapWithLinks(data, remoteUrl);
    console.log(JSON.stringify(wrapped));
  } catch (err) {
    console.log(
      JSON.stringify([
        `Error connecting to ${remoteUrl}: ${err instanceof Error ? err.message : String(err)}`,
      ]),
    );
  }
}

main();
