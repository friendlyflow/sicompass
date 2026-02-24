// Sales demo provider for sicompass
// Run with: bun run sales_demo.ts <path>
// Outputs JSON array of children at the given path to stdout

interface Section {
  key: string;
  children: (string | Section)[];
}

const sections: Section[] = [
  {
    key: "Welcome",
    children: [
      "Sicompass — a keyboard-driven interface for navigating structured information.",
      "Everything you see here is a live, navigable tree.",
      "Use j/k or arrow keys to move. Press l or Enter to go deeper. h to go back.",
      {
        key: "What Makes It Different",
        children: [
          "No mouse required — pure keyboard navigation throughout.",
          "Providers let any data source become a navigable tree.",
          "Modal editing: operator, editor, command, and search modes.",
          "Embeds rich content: images, checkboxes, radio groups, live links.",
          "Lightweight — built in C with Vulkan rendering.",
        ],
      },
    ],
  },
  {
    key: "Key Features",
    children: [
      {
        key: "Navigation",
        children: [
          "h / Left — go up to parent level",
          "j / Down — move to next item",
          "k / Up — move to previous item",
          "l / Right / Enter — open selected item",
          "Tab — search/filter items in current view",
          ": — command mode",
        ],
      },
      {
        key: "Modes",
        children: [
          {
            key: "<radio>Active Mode",
            children: [
              "<checked>Operator — navigate, select, act on items",
              "Editor — full-screen text editing",
              "Command — type and execute commands",
              "Search — live-filter the current list",
            ],
          },
        ],
      },
      {
        key: "Rich Content",
        children: [
          "<checkbox checked>Checkboxes with persistent state",
          "<checkbox>Unchecked items waiting for action",
          {
            key: "<radio>Priority",
            children: [
              "Low",
              "<checked>Medium",
              "High",
              "Critical",
            ],
          },
          "<image>textures/texture.jpg</image>",
          "Images render inline within any list.",
          "Links load external JSON or FFON files as child trees.",
        ],
      },
      {
        key: "Providers",
        children: [
          "Providers are plugins that expose any data as a navigable tree.",
          "Built-in: File Browser, Settings, Tutorial.",
          "Custom providers can be scripted in TypeScript and run via Bun.",
          "Providers register commands, handle input, and manage their own state.",
          "Multiple providers can be active simultaneously in a split view.",
        ],
      },
    ],
  },
  {
    key: "Use Cases",
    children: [
      {
        key: "Personal Productivity",
        children: [
          "Hierarchical notes and task lists navigable without leaving the keyboard.",
          "Checkbox-driven to-do trees with drill-down detail.",
          "Quick file system navigation alongside your notes.",
        ],
      },
      {
        key: "Developer Tooling",
        children: [
          "Browse codebases as structured trees.",
          "Attach metadata, notes, or checklists to any file or directory.",
          "Command palette for custom project actions.",
          "Scriptable providers integrate with APIs, databases, or build systems.",
        ],
      },
      {
        key: "Team Dashboards",
        children: [
          "Structured status views navigable by any team member.",
          "Radio-group selections for status tracking (todo / in-progress / done).",
          "Link nodes pull in live data from external JSON endpoints.",
          "Keyboard-first means zero friction for terminal-native teams.",
        ],
      },
      {
        key: "Knowledge Bases",
        children: [
          "Deep hierarchies are first-class — no depth limit.",
          "Full-text search across any level of the tree.",
          "Images and rich formatting inline with text content.",
          "Export or link to external FFON/JSON files for composable data.",
        ],
      },
    ],
  },
  {
    key: "Architecture",
    children: [
      {
        key: "Core",
        children: [
          "Written in C for minimal overhead and maximum portability.",
          "Vulkan-based renderer — hardware-accelerated text and graphics.",
          "FFON: FriendlyFlow Object Notation — the native data format.",
          "UTF-8 throughout with HarfBuzz + FreeType text shaping.",
        ],
      },
      {
        key: "Provider System",
        children: [
          "Providers implement a small interface: getChildren, handleInput, getCommands.",
          "ProviderOps: a simplified API for plugin authors.",
          "Generic implementations handle boilerplate — authors focus on data.",
          "Providers communicate via lib_provider — no Sicompass internals exposed.",
        ],
      },
      {
        key: "Script Providers",
        children: [
          "TypeScript scripts run via Bun as child processes.",
          "A script receives a path string and writes JSON children to stdout.",
          "Zero runtime dependency on the host app — any language works.",
          "This demo is itself a script provider.",
        ],
      },
    ],
  },
  {
    key: "Roadmap",
    children: [
      {
        key: "Near Term",
        children: [
          "<checkbox checked>File browser with clipboard (cut/copy/paste)",
          "<checkbox checked>Settings provider with live theme switching",
          "<checkbox checked>Script providers via Bun",
          "<checkbox>Collaborative real-time trees",
          "<checkbox>Plugin marketplace",
        ],
      },
      {
        key: "Longer Term",
        children: [
          "<checkbox>Mobile companion app",
          "<checkbox>Cloud sync for trees and settings",
          "<checkbox>AI-assisted tree generation",
          "<checkbox>Embedded WASM provider runtime",
        ],
      },
    ],
  },
  {
    key: "Get Started",
    children: [
      "Build from source: meson setup build && ninja -C build",
      "Run: ./build/sicompass",
      "Navigate to Settings to configure your color scheme and providers.",
      "Add a script provider by pointing to any .ts file that follows the protocol.",
      {
        key: "Resources",
        children: [
          "<link>assets/sf.json</link>",
          "Source code: github.com/friendlyflow/sicompass",
          "Docs: see the Tutorial provider (built in to sicompass).",
        ],
      },
      {
        key: "Contact",
        children: [
          "Questions? Reach us at hello@friendlyflow.com",
          "Issues and feature requests: github.com/friendlyflow/sicompass/issues",
        ],
      },
    ],
  },
];

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
