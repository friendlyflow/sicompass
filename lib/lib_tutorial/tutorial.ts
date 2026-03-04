// Tutorial provider for sicompass
// Run with: bun run tutorial.ts <path>
// Outputs JSON array of children at the given path to stdout

interface Section {
  key: string;
  children: (string | Section)[];
}

const sections: Section[] = [
  {
    key: "Welcome --> use Right key, Down, Up and Left key",
    children: [
      "Sicompass is a keyboard-driven interface for browsing and managing structured data.",
      "It unifies file browsing, chat, email, and settings into a single navigable tree.",
      "Each top-level item is a program (provider) that plugs into the same interface.",
    ],
  },
  {
    key: "Navigation",
    children: [
      {
        key: "Moving Around",
        children: [
          "Left key: go back (parent level)",
          "Down key: move down in list",
          "Up key: move up in list",
          "Right key / Enter: go into selected item",
        ],
      },
      {
        key: "Modes",
        children: [
          "Space: toggle between operator and editor mode",
          ": (colon): command mode - type commands",
          "Tab: simple search mode - filter items in current view",
          "Tab again: scroll mode - scroll in a long text body",
          "Ctrl + f: extended search mode - filtem items in children",
        ],
      },
    ],
  },
  {
    key: "Editing",
    children: [
      "Press i to enter insert mode on an editable item.",
      "Press a to enter append mode on an editable item.",
      "Press Escape to return to the previous mode.",
      "Press Enter to confirm your edit.",
    ],
  },
  {
    key: "Commands",
    children: [
      "Press : (colon) to enter command mode.",
      ":create file - create a new file (in file browser)",
      ":create directory - create a new directory",
    ],
  },
  {
    key: "Programs",
    children: [
      "Programs (providers) appear as top-level items at the root.",
      "Configure which programs to load in Settings.",
      {
        key: "File Browser",
        children: [
          "Browse your filesystem as a navigable tree.",
          "Rename files and directories with i (inline edit).",
          "Create items with : commands (create file, create directory).",
          "Delete items with the Delete key, copy with Ctrl+C, paste with Ctrl+V.",
        ],
      },
      {
        key: "Sales Demo",
        children: [
          "An interactive air handling unit (HVAC) product configurator.",
          "Navigate supply and return air components like filters, coils, fans, and recovery wheels.",
          "Edit parameters such as temperatures, pressures, and dimensions inline.",
          "Add optional components (chillers, fan coil units) via 'Add element:' sections.",
          "Press 'd' at the root to view the technical unit diagram.",
        ],
      },
      {
        key: "Chat Client (not yet functional)",
        children: [
          "A Matrix protocol chat client.",
          "Lists rooms and messages as a navigable tree.",
          "Send messages by editing inline within a room.",
          "Configure homeserver URL and credentials in Settings.",
        ],
      },
      {
        key: "Email Client (not yet functional)",
        children: [
          "Read and send email via IMAP and SMTP.",
          "Supports Google OAuth2 for Gmail accounts.",
          "Folders and messages appear as a navigable tree.",
          "Configure server URLs and credentials in Settings.",
        ],
      },
      {
        key: "Settings",
        children: [
          "The settings provider is always loaded last in the root.",
          "Contains radio groups and text inputs for configuration.",
          "Color scheme (dark/light) is configured here.",
        ],
      },
    ],
  },
  {
    key: "Interactive Elements",
    children: [
      "This section demonstrates interactive element types.",
      "<checkbox checked>Try toggling this checkbox",
      "<checkbox>And this unchecked one",
      {
        key: "<radio>Pick a color",
        children: [
          "<checked>blue",
          "green",
          "red",
        ],
      },
      "<image>textures/texture.jpg</image>",
      "Links load external JSON or FFON files as children:",
      {
        key: "<link>assets/sf.json</link>",
        children: [],
      },
    ],
  },
  {
    key: "Configuration",
    children: [
      "Ctrl+S: save the current provider's configuration.",
      "Ctrl+Shift+S: save as (enter a filename).",
      "Ctrl+O: open/load a configuration file.",
      "Config is stored in ~/.config/sicompass/settings.json.",
      "The programsToLoad array controls which providers are loaded.",
    ],
  },
  {
    key: "Development",
    children: [
      {
        key: "Provider Types",
        children: [
          "C Provider (ProviderOps): implement a ProviderOps struct, call providerCreate(ops)",
          "Script Provider: write a TypeScript file, loaded via scriptProviderCreate(name, displayName, scriptPath)",
          "Factory Provider: register with providerFactoryRegister(name, createFn), instantiate by name",
        ],
      },
      {
        key: "ProviderOps Functions",
        children: [
          {
            key: "Data",
            children: [
              "fetch: return an array of FFON elements for the current path (required)",
              "commitEdit: save an inline edit (e.g. rename a file or change a setting value)",
              "dashboardImagePath: set a path to an image shown fullscreen via 'd' key",
            ],
          },
          {
            key: "Lifecycle",
            children: [
              "init: called once at startup before any operations",
              "cleanup: called at shutdown to free resources",
              "loadConfig: load persistent configuration from a file path",
              "saveConfig: save persistent configuration to a file path",
            ],
          },
          {
            key: "Navigation",
            children: [
              "pushPath: append a segment to the current path (go deeper)",
              "popPath: remove the last segment from the current path (go back)",
              "getCurrentPath: return the current path string",
              "setCurrentPath: jump directly to an absolute path (teleport after search)",
            ],
          },
          {
            key: "File Operations",
            children: [
              "createDirectory: create a new directory at the current path",
              "createFile: create a new file at the current path",
              "deleteItem: delete a file or directory (recursively for non-empty dirs)",
              "copyItem: copy a file or directory from source to destination",
            ],
          },
          {
            key: "Commands",
            children: [
              "getCommands: return a list of command names this provider supports",
              "handleCommand: prepare/validate a command and optionally return a UI element",
              "getCommandListItems: return a list of selectable options for a command",
              "executeCommand: execute a command with the user's selected option",
            ],
          },
          {
            key: "Events",
            children: [
              "onRadioChange: called when a radio group selection changes",
              "onButtonPress: called when a \\<button> element is activated",
              "createElement: create a new FFON element for 'Add element:' sections",
            ],
          },
          {
            key: "Search",
            children: [
              "collectDeepSearchItems: return all searchable items for extended search (Tab key)",
              "If not implemented, the system falls back to FFON-tree traversal",
            ],
          },
        ],
      },
      {
        key: "Element Tags",
        children: [
          "Use \\\\< and \\\\> to escape angle brackets in text",
          "\\<input>content\\</input> - make an element editable inline",
          "\\<radio>group name - parent object for mutually exclusive options",
          "\\<checked>option - mark a radio option as selected",
          "\\<checkbox>label - unchecked boolean toggle",
          "\\<checkbox checked>label - checked boolean toggle",
          "\\<link>path/to/file.json\\</link> - lazy-load external JSON/FFON as children",
          "\\<image>path/to/image.jpg\\</image> - display an image",
          "\\<button>functionName\\</button>Display Text - clickable button element",
          "\\<opt>\\</opt>key - many-option element, deletable after creation",
          "\\<one-opt>\\</one-opt>key - single-use button, replaced after creation",
        ],
      },
    ],
  },
  {
    key: "Next Steps",
    children: [
      "Sicompass is growing. Here's what's planned:",
      "Web Browser — accessible web browsing within sicompass.",
      "Store — an app ecosystem where others can build and publish providers.",
      "Notebook — structured note-taking with server-side sync.",
      "IDE — code as a navigable structure, with C code generation.",
      "Terminal — a terminal emulator integrated as a provider.",
      "Blog — publish content, with paid access, viewable in browsers too.",
      "Mobile — Android and iOS versions.",
      "Contributions welcome! Join us on Discord.",
      "Happy navigating!",
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

// Parse path: "/" → [], "/Welcome" → ["Welcome"], "/Navigation/Modes" → ["Navigation", "Modes"]
const rawPath = process.argv[2] || "/";
const pathParts = rawPath === "/" ? [] : rawPath.split("/").filter(Boolean);

const children = getChildrenAtPath(sections, pathParts);
if (children) {
  console.log(JSON.stringify(toJson(children)));
} else {
  console.log("[]");
}
