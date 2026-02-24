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

// ─── Equipment JSON converter ────────────────────────────────────────────────

const CARDINALITY = new Set(["one mand", "one opt", "many opt", "many mand"]);

function isCardinality(v: unknown): v is string {
  return typeof v === "string" && CARDINALITY.has(v);
}

function convertEquipmentEntry(key: string, raw: unknown[]): string | Section {
  if (raw.length === 0) return key;

  // Format A: ["cardinality", content?]
  if (isCardinality(raw[0])) {
    const content = raw[1];
    if (content === undefined || content === null) return key;

    if (Array.isArray(content)) {
      // Radio/select options list
      return { key, children: content.map(String) };
    }
    if (typeof content === "string") {
      return { key, children: [content] };
    }
    if (typeof content === "object") {
      const rawEntries = Object.entries(content as Record<string, unknown[]>);
      const children = rawEntries.map(([k, v]) => convertEquipmentEntry(k, v));

      const addOptions: string[] = [];
      for (const [k, v] of rawEntries) {
        const card = v[0];
        if (card === "many opt") {
          addOptions.push(k);
        } else if (card === "one opt" && v[1] === undefined) {
          addOptions.push(k);
        }
      }

      if (addOptions.length > 0) {
        children.push({ key: "<radio>Add element:", children: addOptions });
      }

      return { key, children };
    }
    return key;
  }

  // Format B: [[cardinality, value], ...] – list of pairs (e.g. "version")
  if (Array.isArray(raw[0])) {
    const children: string[] = [];
    for (const entry of raw) {
      if (
        Array.isArray(entry) &&
        isCardinality(entry[0]) &&
        typeof entry[1] === "string"
      ) {
        children.push(entry[1]);
      }
    }
    return children.length > 0 ? { key, children } : key;
  }

  return key;
}

// Load equipment1.json relative to this script file (top-level await, Bun supports this)
const scriptDir = new URL(".", import.meta.url).pathname;
const equipmentRaw = await Bun.file(
  scriptDir + "../../assets/equipment1.json"
).json() as Record<string, unknown[]>;

const rootEntries = Object.entries(equipmentRaw);

const sections: (string | Section)[] = rootEntries
  .filter(([_k, v]) => typeof v[0] === "string" && v[0].includes("mand"))
  .map(([k, v]) => convertEquipmentEntry(k, v));

const rootAddOptions = rootEntries
  .filter(([_k, v]) => {
    const card = v[0];
    return card === "many opt" || (card === "one opt" && v[1] === undefined);
  })
  .map(([k]) => k);

if (rootAddOptions.length > 0) {
  sections.push({ key: "<radio>Add element:", children: rootAddOptions });
}

function getChildrenAtPath(
  nodes: (string | Section)[],
  pathParts: string[]
): (string | Section)[] | null {
  if (pathParts.length === 0) {
    return nodes;
  }

  const [head, ...rest] = pathParts;
  for (const node of nodes) {
    if (typeof node !== "string" && node.key === head) {
      return getChildrenAtPath(node.children, rest);
    }
  }

  return null;
}

function toJson(children: (string | Section)[]): unknown[] {
  return children.map((child) => {
    if (typeof child === "string") {
      return child;
    }
    return { [child.key]: toJson(child.children) };
  });
}

// Parse path: "/" → [], "/Welcome" → ["Welcome"], "/Key Features/Modes" → ["Key Features", "Modes"]
const rawPath = process.argv[2] || "/";
const pathParts = rawPath === "/" ? [] : rawPath.split("/").filter(Boolean);

const children = getChildrenAtPath(sections, pathParts);
if (children) {
  console.log(JSON.stringify(toJson(children)));
} else {
  console.log("[]");
}
