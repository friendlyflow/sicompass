// Sales demo provider for sicompass
// Run with: bun run sales_demo.ts <path>
// Outputs JSON array of children at the given path to stdout

export {}; // make this file a module so top-level await and declarations work

// Bun global – declared so TypeScript doesn't complain in environments without @types/bun
declare const Bun: { file: (path: string) => { json(): Promise<unknown> } };

interface Section {
  key: string;
  children: (string | Section)[];
}

// ─── Equipment JSON helpers ───────────────────────────────────────────────────

const CARDINALITY = new Set(["one mand", "one opt", "many opt"]);

function isCardinality(v: unknown): v is string {
  return typeof v === "string" && CARDINALITY.has(v);
}

// Navigate the raw equipment JSON to the object at pathParts.
// Returns null if the path is not found or the target is not a nested object.
function getRawAtPath(
  raw: Record<string, unknown[]>,
  pathParts: string[]
): Record<string, unknown[]> | null {
  if (pathParts.length === 0) return raw;
  const [head, ...rest] = pathParts;
  const entry = raw[head];
  if (!entry || !isCardinality(entry[0])) return null;
  const content = entry[1];
  if (content === null || content === undefined || typeof content !== "object" || Array.isArray(content)) return null;
  return getRawAtPath(content as Record<string, unknown[]>, rest);
}

// Build the display children for a raw JSON object:
// mandatory items are shown directly; optional items go into "Add element:".
function buildDisplayChildren(rawObj: Record<string, unknown[]>): (string | Section)[] {
  const result: (string | Section)[] = [];
  const addItems: string[] = [];

  for (const [k, v] of Object.entries(rawObj)) {
    const card = v[0];
    if (!isCardinality(card)) continue;
    if ((card as string).includes("opt")) {
      const prefix = card === "one opt" ? "one-opt:" : "";
      addItems.push(`<button>${prefix}${k}</button>${k}`);
    } else {
      result.push(buildItem(k, v));
    }
  }

  if (addItems.length > 0) {
    result.push({ key: "Add element:", children: addItems });
  }

  return result;
}

// Convert a single equipment entry to its display item.
function buildItem(key: string, raw: unknown[]): string | Section {
  if (raw.length === 0) return key;

  // Format A: ["cardinality", content?]
  if (isCardinality(raw[0])) {
    const content = raw[1];
    if (content === undefined || content === null) return key;
    if (Array.isArray(content)) return { key, children: content.map(String) };
    if (typeof content === "string") return { key, children: [content] };
    if (typeof content === "object") {
      return { key, children: buildDisplayChildren(content as Record<string, unknown[]>) };
    }
    return key;
  }

  // Format B: [[cardinality, value], ...] – list of pairs (e.g. "version")
  if (Array.isArray(raw[0])) {
    const children: string[] = [];
    for (const entry of raw) {
      if (Array.isArray(entry) && isCardinality(entry[0]) && typeof entry[1] === "string") {
        children.push(entry[1]);
      }
    }
    return children.length > 0 ? { key, children } : key;
  }

  return key;
}

function toJson(children: (string | Section)[]): unknown[] {
  return children.map((child) => {
    if (typeof child === "string") return child;
    return { [child.key]: toJson(child.children) };
  });
}

// ─── Main ─────────────────────────────────────────────────────────────────────

// On Windows, URL.pathname returns "/C:/..." — strip the leading slash so file paths work
let scriptDir = new URL(".", import.meta.url).pathname;
if (/^\/[A-Za-z]:\//.test(scriptDir)) scriptDir = scriptDir.slice(1);
const equipmentRaw = await Bun.file(
  scriptDir + "assets/equipment1.json"
).json() as Record<string, unknown[]>;

// Parse path: "/" → [], "/Welcome" → ["Welcome"], "/Key Features/Modes" → ["Key Features", "Modes"]
const rawPath = process.argv[2] || "/";
const pathParts = rawPath === "/" ? [] : rawPath.split("/").filter(Boolean);

const dashboardImage = scriptDir + "assets/115-Draw-through-Air-Handling-Unit-Diagram-1.webp";

const rawObj = getRawAtPath(equipmentRaw, pathParts);
const children = rawObj ? buildDisplayChildren(rawObj) : null;
if (children) {
  const jsonChildren = toJson(children);
  if (pathParts.length === 0) {
    console.log(JSON.stringify({ children: jsonChildren, dashboardImage }));
  } else {
    console.log(JSON.stringify(jsonChildren));
  }
} else {
  console.log("[]");
}
