// Tutorial provider for sicompass
// Run with: bun run tutorial.ts <path>
// Outputs JSON array of children at the given path to stdout

interface Section {
  key: string;
  children: (string | Section)[];
}

const sections: Section[] = [
  {
    key: "Welcome",
    children: [
      "Sicompass is a keyboard-driven navigable structure.",
      "Use j/k or arrows to move up and down in this list.",
      "Press Enter or l to go deeper. Press Escape or h to go back.",
    ],
  },
  {
    key: "Navigation",
    children: [
      {
        key: "Moving Around",
        children: [
          "h or Left Arrow: go back (parent level)",
          "j or Down Arrow: move down in list",
          "k or Up Arrow: move up in list",
          "l or Right Arrow / Enter: go into selected item",
        ],
      },
      {
        key: "Modes",
        children: [
          "o: operator mode - navigate and perform actions",
          "e: editor mode - edit text content",
          ":: command mode - type commands",
          "Tab: search mode - filter items in current view",
        ],
      },
    ],
  },
  {
    key: "Editing",
    children: [
      "Press i to enter insert mode on an editable item.",
      "Press a to enter append mode.",
      "Press Escape to return to operator mode.",
      "Press Enter to confirm your edit.",
    ],
  },
  {
    key: "Commands",
    children: [
      "Press : to enter command mode.",
      ":create file - create a new file (in file browser)",
      ":create directory - create a new directory",
      ":editor mode - switch to editor mode",
      ":operator mode - switch to operator mode",
    ],
  },
  {
    key: "File Browser",
    children: [
      "The file browser is another provider below this tutorial.",
      "Navigate into it to browse your filesystem.",
      "You can rename files and directories with insert mode.",
      "You can create files and directories with : commands.",
    ],
  },
  {
    key: "Next Steps",
    children: [
      "Press Escape or h to go back to the root.",
      "Navigate down to the file browser to explore your files.",
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
    return { [child.key]: [] };
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
