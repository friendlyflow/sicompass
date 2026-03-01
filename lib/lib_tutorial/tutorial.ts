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
      "<checkbox checked>checkbox 1",
      "<checkbox>checkbox 2",
      {
        key: "<radio>radio parent",
        children: [
          "<checked>item 1",
          "item 2",
          "item 3",
        ],
      },
      {
        key: "<radio>radio parent wrong",
        children: [
          "<checked>item 1",
          "item 2",
          "item 3",
          "<checked>item 111",
        ],
      },
      "<image>textures/texture.jpg</image>",
      "<image>textures/texture.jpg</image>",
      "Sicompass is a keyboard-driven navigable structure.",
      "Use j/k or arrows to move up and down in this list.",
      "Press Enter or l to go deeper. Press Escape or h to go back.",
      "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.\nPellentesque habitant morbi tristique senectus et netus et malesuada fames ac turpis egestas. Sed tempus urna et pharetra pharetra massa tincidunt nunc pulvinar. Sapien et ligula ullamcorper malesuada proin nibh nisl condimentum id. Venenatis a condimentum vitae sapien pellentesque habitant morbi tristique senectus.\nEt netus et malesuada fames ac turpis egestas maecenas pharetra. Convallis posuere ac ut consequat semper viverra nam libero justo. Laoreet sit amet cursus sit amet dictum sit amet justo. Donec enim nec dui nunc mattis enim ut tellus elementum.\nSagittis vitae turpis massa sed elementum tempus egestas sed sed. Risus pretium quam vulputate dignissim suspendisse in est ante in. Nibh mauris commodo quis imperdiet massa tincidunt nunc pulvinar sapien.\nEt ligula ullamcorper malesuada proin libero nunc consequat interdum varius. Quam quisque id diam vel quam elementum pulvinar etiam non. Curabitur gravida arcu ac tortor dignissim convallis aenean et molestie.\nAc feugiat sed lectus vestibulum mattis ullamcorper velit egestas dui. Id ornare arcu odio ut sem nulla pharetra et ultrices. Neque ornare aenean euismod elementum nisi quis enim lobortis scelerisque.\nFermentum dui faucibus in ornare quam viverra accumsan in nisl. Nisi scelerisque eu ultrices vitae auctor eu augue ut lectus. Arcu bibendum at varius vel pharetra vel turpis nunc eget.\nLorem dolor sed viverra ipsum nunc aliquet bibendum enim facilisis. Gravida neque convallis a cras semper auctor neque vitae tempus. Quam pellentesque nec nam aliquam sem et tortor consequat id.\nPorta nibh venenatis cras adipiscing enim eu turpis egestas pretium. Aenean pharetra magna etiam tempor orci eu lobortis elementum nibh. Tellus molestie nunc vel risus commodo viverra maecenas accumsan.\nLacus vel facilisis magna etiam tempor orci eu lobortis elementum. Nibh tellus molestie nunc non blandit massa enim nec dui. Nunc mattis enim ut tellus elementum sagittis vitae et leo.\nDuis ut tortor pretium viverra suspendisse potenti nullam ac tortor. Vitae purus gravida quis blandit turpis cursus in hac habitasse. Platea dictumst quisque sagittis purus sit amet volutpat consequat mauris.\nNunc congue nisi vitae suscipit tellus mauris a diam maecenas. Sed augue lacus viverra vitae congue eu consequat ac felis. Donec et odio pellentesque diam volutpat commodo sed egestas egestas.\nInteger eget aliquet nibh praesent tristique magna sit amet purus. In mollis nunc sed id semper risus in hendrerit gravida. Neque convallis a cras semper auctor neque vitae tempus quam.\nPellentesque nec nam aliquam sem et tortor consequat id porta. Nibh venenatis cras sed felis eget velit aliquet sagittis id. Consectetur a erat nam at lectus urna duis convallis convallis.\nTellus in hac habitasse platea dictumst vestibulum rhoncus est. Pellentesque pulvinar pellentesque habitant morbi tristique senectus et netus et. Malesuada fames ac turpis egestas integer eget aliquet nibh praesent.\nTristique magna sit amet purus gravida quis blandit turpis cursus. In hac habitasse platea dictumst quisque sagittis purus sit amet. Volutpat consequat mauris nunc congue nisi vitae suscipit tellus.\nMauris a diam maecenas sed augue lacus viverra vitae congue. Eu consequat ac felis donec et odio pellentesque diam volutpat. Commodo sed egestas egestas fringilla phasellus faucibus scelerisque eleifend.\nDonec pretium vulputate sapien nec sagittis aliquam malesuada bibendum arcu. Vitae elementum curabitur vitae nunc sed velit dignissim sodales ut. Eu sem integer vitae justo eget magna fermentum iaculis eu.\nNon diam phasellus vestibulum lorem sed risus ultricies tristique nulla. Aliquet bibendum enim facilisis gravida neque convallis a cras semper. Auctor neque vitae tempus quam pellentesque nec nam aliquam sem.\nEt tortor consequat id porta nibh venenatis cras sed felis. Eget velit aliquet sagittis id consectetur a erat nam at. Lectus urna duis convallis convallis tellus id interdum velit laoreet.\nId donec ultrices tincidunt arcu non sodales neque sodales ut. Etiam dignissim diam quis enim lobortis scelerisque fermentum dui faucibus. In ornare quam viverra orci sagittis eu volutpat odio facilisis.\nMauris sit amet massa vitae tortor condimentum lacinia quis vel. Eros in cursus turpis massa tincidunt dui ut ornare lectus. Sit amet est placerat in egestas erat imperdiet sed euismod.\nNisi est sit amet facilisis magna etiam tempor orci eu. Lobortis elementum nibh tellus molestie nunc non blandit massa enim. Nec dui nunc mattis enim ut tellus elementum sagittis vitae.\nEt leo duis ut diam quam nulla porttitor massa id. Neque volutpat ac tincidunt vitae semper quis lectus nulla at. Volutpat diam ut venenatis tellus in metus vulputate eu scelerisque.\nFelis imperdiet proin fermentum leo vel orci porta non pulvinar. Neque laoreet suspendisse interdum consectetur libero id faucibus nisl tincidunt. Eget nulla facilisi etiam dignissim diam quis enim lobortis scelerisque.\nFermentum dui faucibus in ornare quam viverra orci sagittis eu. Volutpat odio facilisis mauris sit amet massa vitae tortor. Condimentum lacinia quis vel eros in cursus turpis massa tincidunt.\nDui ut ornare lectus sit amet est placerat in egestas. Erat imperdiet sed euismod nisi est sit amet facilisis magna. Etiam tempor orci eu lobortis elementum nibh tellus molestie nunc.\nNon blandit massa enim nec dui nunc mattis enim ut. Tellus elementum sagittis vitae et leo duis ut diam quam. Nulla porttitor massa id neque volutpat ac tincidunt vitae semper.\nQuis lectus nulla at volutpat diam ut venenatis tellus in. Metus vulputate eu scelerisque felis imperdiet proin fermentum leo vel. Orci porta non pulvinar neque laoreet suspendisse interdum consectetur.\nLibero id faucibus nisl tincidunt eget nulla facilisi etiam dignissim. Diam quis enim lobortis scelerisque fermentum dui faucibus in ornare. Quam viverra orci sagittis eu volutpat odio facilisis mauris sit.\nAmet massa vitae tortor condimentum lacinia quis vel eros donec. Ac turpis egestas integer eget aliquet nibh praesent tristique magna. Sit amet purus gravida quis blandit turpis cursus in hac.\nHabitasse platea dictumst quisque sagittis purus sit amet volutpat. Consequat mauris nunc congue nisi vitae suscipit tellus mauris a. Diam maecenas sed augue lacus viverra vitae congue eu consequat.\nAc felis donec et odio pellentesque diam volutpat commodo sed. Egestas egestas fringilla phasellus faucibus scelerisque eleifend donec pretium. Vulputate sapien nec sagittis aliquam malesuada bibendum arcu vitae.\nElementum curabitur vitae nunc sed velit dignissim sodales ut eu. Sem integer vitae justo eget magna fermentum iaculis eu non. Diam phasellus vestibulum lorem sed risus ultricies tristique nulla aliquet.\nBibendum enim facilisis gravida neque convallis a cras semper auctor. Neque vitae tempus quam pellentesque nec nam aliquam sem et. Tortor consequat id porta nibh venenatis cras sed felis eget.\nVelit aliquet sagittis id consectetur a erat nam at lectus. Urna duis convallis convallis tellus id interdum velit laoreet id. Donec ultrices tincidunt arcu non sodales neque sodales ut etiam.\nDignissim diam quis enim lobortis scelerisque fermentum dui faucibus in. Ornare quam viverra orci sagittis eu volutpat odio facilisis mauris. Sit amet massa vitae tortor condimentum lacinia quis vel eros.\nIn cursus turpis massa tincidunt dui ut ornare lectus sit. Amet est placerat in egestas erat imperdiet sed euismod nisi. Est sit amet facilisis magna etiam tempor orci eu lobortis.\nElementum nibh tellus molestie nunc non blandit massa enim nec. Dui nunc mattis enim ut tellus elementum sagittis vitae et. Leo duis ut diam quam nulla porttitor massa id neque.\nVolutpat ac tincidunt vitae semper quis lectus nulla at volutpat. Diam ut venenatis tellus in metus vulputate eu scelerisque felis. Imperdiet proin fermentum leo vel orci porta non pulvinar neque.\nLaoreet suspendisse interdum consectetur libero id faucibus nisl tincidunt eget. Nulla facilisi etiam dignissim diam quis enim lobortis scelerisque. Fermentum dui faucibus in ornare quam viverra orci sagittis eu.\nVolutpat odio facilisis mauris sit amet massa vitae tortor lacinia. Condimentum quis vel eros in cursus turpis massa tincidunt dui. Ut ornare lectus sit amet est placerat in egestas erat.\nImperdiet sed euismod nisi est sit amet facilisis magna etiam. Tempor orci eu lobortis elementum nibh tellus molestie nunc non. Blandit massa enim nec dui nunc mattis enim ut tellus.\nElementum sagittis vitae et leo duis ut diam quam nulla. Porttitor massa id neque volutpat ac tincidunt vitae semper quis. Lectus nulla at volutpat diam ut venenatis tellus in metus.\nVulputate eu scelerisque felis imperdiet proin fermentum leo vel orci. Porta non pulvinar neque laoreet suspendisse interdum consectetur libero. Id faucibus nisl tincidunt eget nulla facilisi etiam dignissim diam.\nQuis enim lobortis scelerisque fermentum dui faucibus in ornare quam. Viverra orci sagittis eu volutpat odio facilisis mauris sit amet. Massa vitae tortor condimentum lacinia quis vel eros in cursus.\nTurpis massa tincidunt dui ut ornare lectus sit amet est. Placerat in egestas erat imperdiet sed euismod nisi est sit. Amet facilisis magna etiam tempor orci eu lobortis elementum nibh.\nTellus molestie nunc non blandit massa enim nec dui nunc. Mattis enim ut tellus elementum sagittis vitae et leo duis. Ut diam quam nulla porttitor massa id neque volutpat ac.",
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
          "Space: toggle between operator and editor mode",
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
      "Press Escape to return to the previous mode.",
      "Press Enter to confirm your edit.",
    ],
  },
  {
    key: "Commands",
    children: [
      "Press : to enter command mode.",
      ":create file - create a new file (in file browser)",
      ":create directory - create a new directory",
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
    key: "Links",
    children: [
      "Links load external JSON or FFON files as children.",
      {
        key: "<link>assets/sf.json</link>",
        children: [],
      },
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
        key: "Data & Fetching",
        children: [
          "fetch: return an array of FFON elements for the current path (required)",
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
        key: "Editing",
        children: [
          "commitEdit: save an inline edit (e.g. rename a file or change a setting value)",
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
        key: "Command System",
        children: [
          "getCommands: return a list of command names this provider supports",
          "handleCommand: prepare/validate a command and optionally return a UI element",
          "getCommandListItems: return a list of selectable options for a command",
          "executeCommand: execute a command with the user's selected option",
        ],
      },
      {
        key: "Event Handlers",
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
