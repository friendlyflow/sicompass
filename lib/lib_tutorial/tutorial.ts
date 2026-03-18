// Tutorial provider for sicompass
// Run with: bun run tutorial.ts <path>
// Outputs JSON array of children at the given path to stdout

interface Section {
  key: string;
  children: (string | Section)[];
}

function makeLoremIpsum(): string {
  const base =
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. " +
    "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. " +
    "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. " +
    "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. " +
    "Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium. ";
  return Array.from({ length: 10 }, () => base).join("");
}

const sections: Section[] = [
  {
    key: "Welcome --> here you can go up, down, right or left",
    children: [
      "Welcome to Sicompass, a keyboard-driven interface for browsing and managing structured data.",
      "Every graphical interface is essentially a tree of lists. Sicompass makes that structure explicit and navigable entirely from the keyboard, eliminating the need for a mouse.",
      "This means one consistent way to navigate everything: files, documents, settings, web pages, and any data source built on top of it.",
      "Each top-level item you see at the root is a program (also called a provider). Providers plug into the same unified interface, so once you learn to navigate one, you can navigate them all.",
      "This tutorial is itself a provider. Everything you learn here, you're practicing right now by navigating through it.",
      "Use the Right arrow key to dive into any section, and Left to come back. Let's get started!",
    ],
  },
  {
    key: "Navigation",
    children: [
      "Navigation in Sicompass works like a file manager: you move through a tree of items using the arrow keys. Every provider, whether it's a file browser, web browser, or this tutorial, uses the same navigation model.",
      {
        key: "Moving Around",
        children: [
          "Up key: move the selection up in the current list",
          "Down key: move the selection down in the current list",
          "Right key: go into the selected item (descend into its children)",
          "Left key: go back to the parent level (ascend one level up)",
          "Enter: confirm or activate the selected item (e.g. toggle a checkbox, open a link)",
          "These five keys are all you need to navigate any content in Sicompass. The tree can be arbitrarily deep. Just keep pressing Right to go deeper, and Left to come back.",
        ],
      },
      {
        key: "Modes",
        children: [
          "Sicompass has several modes that change what your keyboard inputs do. You always start in operator mode.",
          "Space: toggle between operator mode and editor mode. Operator mode is for navigating; editor mode enables additional editing shortcuts.",
          ": (colon): enter command mode. Type a command name and press Enter to execute it. Commands are context-sensitive, each provider can offer its own commands.",
          "Tab: enter simple search mode. Start typing to filter items in the current list. Only items matching your search will be shown.",
          "Tab again (from search mode): switch to scroll mode. Use Up/Down to scroll through a long text body without moving the selection.",
          "Ctrl+F: enter extended search mode. This searches recursively through all children, not just the current level. Results are shown as a flat list you can jump to.",
          "Escape: go back to the previous mode, or cancel the current operation.",
        ],
      },
    ],
  },
  {
    key: "Accessibility",
    children: [
      "Sicompass has built-in screen reader support powered by AccessKit. If you use a screen reader, Sicompass works with it out of the box — no configuration needed.",
      "Screen reader support is available on all platforms: Linux (AT-SPI), macOS (VoiceOver), and Windows (Narrator, NVDA, JAWS).",
      "When you navigate up, down, or into items, the current element is automatically announced by your screen reader.",
      "Mode changes are also announced. For example, switching to insert mode announces 'editor insert', entering search announces 'search', and returning to normal navigation announces 'operator mode'.",
      "Screen reader support activates automatically when a screen reader is detected. There is nothing to enable or configure.",
    ],
  },
  {
    key: "Editing",
    children: [
      "Some items in Sicompass are editable. You can tell because they contain an <input> tag. For example, file names in the file browser or setting values can be edited inline.",
      "Press i to enter insert mode. Your cursor is placed at the beginning of the editable text, and you can type to replace or modify it.",
      "Press a to enter append mode. Your cursor is placed at the end of the editable text, so you can add to what's already there.",
      "While editing, type normally to change the text. Use Backspace to delete characters.",
      "Press Enter to confirm your edit and save the change.",
      "Press Escape to cancel the edit and discard your changes.",
      "Not all items are editable, only those marked with <input> tags by the provider. The file browser makes file and directory names editable; the settings provider makes configuration values editable.",
    ],
  },
  {
    key: "Commands",
    children: [
      "Commands let you perform actions beyond simple navigation and editing. Each provider can define its own set of commands.",
      "Press : (colon) to enter command mode. A command prompt appears at the bottom of the screen.",
      "Start typing the command name. Matching commands will appear as suggestions. Press Enter to select one.",
      "Some commands take additional input. For example, 'create file' in the file browser will prompt you for a filename.",
      "Common file browser commands:",
      ":create file - create a new file in the current directory",
      ":create directory - create a new directory in the current directory",
      "Providers can define any commands they want. Check each provider's command list by pressing : and browsing the suggestions.",
    ],
  },
  {
    key: "Programs",
    children: [
      "Programs (also called providers) are the building blocks of Sicompass. Each program turns a different data source into the same navigable tree structure.",
      "Programs appear as top-level items when you navigate to the root (press Left until you can't go further).",
      "You can configure which programs are loaded in Settings under 'Available programs'. Enable or disable them at any time, changes take effect instantly.",
      {
        key: "File Browser",
        children: [
          "The file browser turns your filesystem into a navigable tree. Directories become sections you can enter with the Right key; files are leaf items.",
          "Browse your filesystem by navigating up, down, and into directories. The current path is shown at the top of the screen.",
          "Rename files and directories by pressing i (insert mode) on any item. The name becomes editable inline. Type the new name and press Enter to confirm.",
          "Create new items with commands: press : and type 'create file' or 'create directory'.",
          "Delete items by pressing the Delete key on a selected item. Directories are deleted recursively if non-empty.",
          "Copy with Ctrl+C and paste with Ctrl+V to duplicate files and directories.",
          "The file browser supports all the standard modes: search (Tab) to filter files, extended search (Ctrl+F) to find files recursively in subdirectories.",
        ],
      },
      {
        key: "Sales Demo",
        children: [
          "The Sales Demo is an interactive air handling unit (HVAC) product configurator. It demonstrates how Sicompass can handle complex, hierarchical data with inline editing.",
          "Navigate supply air and return air sections to explore components like filters, coils, fans, and recovery wheels.",
          "Each component has editable parameters (temperatures, pressures, dimensions, and more) that you can modify inline.",
          "Optional components (chillers, fan coil units) can be added via 'Add element:' sections. These use the <one-opt> and <many-opt> element types.",
          "Press 'd' at the root level to view a technical unit diagram as a fullscreen image. This showcases the dashboardImagePath provider feature.",
          "This provider is a good reference for building data-heavy configurators on top of Sicompass.",
        ],
      },
      {
        key: "Web Browser",
        children: [
          "The web browser lets you browse the internet directly inside Sicompass, turning web pages into keyboard-navigable trees.",
          "At the top level, you'll find an address bar. Press i to edit it, type a URL, and press Enter to load the page.",
          "HTML is automatically converted into a navigable FFON tree based on the page's heading hierarchy (h1-h6). Headings become nested sections; paragraphs, lists, tables, and links are preserved as tree items.",
          "Navigate web content the same way you navigate files or settings. Right to go deeper into a section, Left to go back.",
          "Links on web pages can be followed by selecting them and pressing Enter, which loads the linked page.",
          "This demonstrates how any structured content, even the web, can be unified into the same navigation model.",
        ],
      },
      {
        key: "Plugin Store",
        children: [
          "The Plugin Store lets you manage which providers are active. It appears in Settings under 'Available programs'.",
          "Each provider is shown as a checkbox. Check it to enable the provider, uncheck it to disable it.",
          "Changes take effect immediately. Providers are hot-loaded or unloaded without restarting the app.",
          "Both built-in providers and user-installed plugins appear here. Plugins installed in ~/.config/sicompass/plugins/ are automatically discovered.",
          "This is the easiest way to customize your Sicompass setup. Enable only the programs you use.",
        ],
      },
      {
        key: "Remote Services",
        children: [
          "Sicompass can connect to remote FFON providers served over HTTP, extending the interface beyond your local machine.",
          "Configure a remoteUrl and optional apiKey in Settings to connect to a remote service.",
          "Remote content is lazily fetched as you navigate. Only the data you actually view is downloaded, keeping things fast even with large datasets.",
          "Providers can use the included TypeScript server SDK to build FFON services with optional Stripe or LemonSqueezy billing integration.",
          "This enables SaaS-style products where the entire user interface is delivered through Sicompass's navigable tree.",
        ],
      },
      {
        key: "Chat Client (not yet functional)",
        children: [
          "A planned Matrix protocol chat client for real-time messaging inside Sicompass.",
          "Rooms and messages will appear as a navigable tree. Rooms as sections, messages as items within them.",
          "Send messages by editing inline within a room, using the same i/a editing keys.",
          "Configure homeserver URL and credentials in Settings.",
          "This provider is under development and not yet functional.",
        ],
      },
      {
        key: "Email Client (not yet functional)",
        children: [
          "A planned email client supporting IMAP for reading and SMTP for sending.",
          "Supports Google OAuth2 for seamless Gmail account integration.",
          "Folders (inbox, sent, drafts) and messages will appear as a navigable tree, with message bodies rendered as readable text items.",
          "Configure server URLs and credentials in Settings.",
          "This provider is under development and not yet functional.",
        ],
      },
      {
        key: "Settings",
        children: [
          "The settings provider is always loaded as the last item in the root. It's where you configure Sicompass itself and all loaded providers.",
          "Settings are organized by namespace. 'sicompass' for global settings, and each provider can have its own section.",
          "Color scheme (dark/light) is configured here as a radio group. Changes take effect immediately.",
          "The 'Available programs' section is where you enable and disable providers (see Plugin Store above).",
          "All settings are stored in ~/.config/sicompass/settings.json. You can edit this file directly if you prefer.",
        ],
      },
    ],
  },
  {
    key: "Interactive Elements",
    children: [
      "This section is a hands-on playground for all the interactive element types that Sicompass supports. Try each one as you go!",
      "Checkboxes are boolean toggles. Press Enter on a checkbox to toggle it on or off.",
      "<checkbox checked>Try toggling this checkbox (it starts checked)",
      "<checkbox>And this unchecked one",
      {
        key: "<checkbox checked>Navigable checkbox (go inside with Right key)",
        children: [
          "This is an object checkbox. It can be toggled AND navigated into.",
          "Press Enter to toggle the checkbox state, or press the Right key to view these children.",
          "Object checkboxes are useful when you want a feature toggle that also has sub-settings. For example, enabling a provider while also configuring its options.",
        ],
      },
      {
        key: "<checkbox>Another navigable checkbox (unchecked)",
        children: [
          "Object checkboxes work the same whether checked or unchecked. The checkbox state and the children are independent.",
        ],
      },
      "Text inputs let you edit a value inline. Press i or a on the item below to start editing:",
      "Edit this text --> <input>hello world</input> <-- press i or a",
      {
        key: "<radio>Pick a color",
        children: [
          "<checked>blue",
          "green",
          "red",
        ],
      },
      "Radio groups let you pick exactly one option from a set. Navigate into the radio group above and press Enter on an option to select it. Only one option can be selected at a time.",
      "Images can be displayed inline within the tree. The image below is loaded from a file path:",
      "<image>textures/texture.jpg</image>",
      "Images also support prefix and suffix text around them:",
      "Image with prefix: <image>textures/texture.jpg</image> and suffix",
      "<image>textures/texture.jpg</image> and suffix",
      "Image with prefix: <image>textures/texture.jpg</image>",
      "Links lazy-load external JSON or FFON files as children. Navigate into the link below to load its content:",
      {
        key: "Link with prefix: <link>lib/lib_tutorial/assets/sf.json</link> and suffix",
        children: [],
      },
      "Scroll mode: when a text item is too long to fit on screen, you can scroll through it. Press Tab twice from operator mode to enter scroll mode, then use Up/Down to scroll the text below:",
      makeLoremIpsum(),
    ],
  },
  {
    key: "Configuration",
    children: [
      "Sicompass stores all configuration in a single file: ~/.config/sicompass/settings.json. This file is organized by namespace, each provider can have its own section.",
      {
        key: "Save and Load",
        children: [
          "Some providers support saving and loading configuration files. This is useful for product configurators or any provider that manages persistent state.",
          "Ctrl+S: save the active provider's data to its default config file.",
          "Ctrl+Shift+S: save as, choose a custom filename for the saved configuration.",
          "Ctrl+O: open/load a saved configuration file.",
          "These shortcuts only work if the active provider has enabled config file support. Plugins enable this by adding \"supportsConfigFiles\": true to their plugin.json manifest.",
        ],
      },
      {
        key: "Settings File",
        children: [
          "The main settings file at ~/.config/sicompass/settings.json uses a namespaced JSON format.",
          "Example structure: { \"sicompass\": { \"colorScheme\": \"dark\", \"programsToLoad\": [...] }, \"file browser\": { \"sortOrder\": \"name\" } }",
          "The \"sicompass\" namespace contains global settings like color scheme and which programs to load.",
          "Each provider adds its own namespace for provider-specific settings.",
          "You can edit this file directly, but changes require a restart to take effect. Using the Settings provider inside Sicompass applies changes immediately.",
        ],
      },
      {
        key: "Plugin Configuration",
        children: [
          "User plugins are installed in ~/.config/sicompass/plugins/<plugin-name>/.",
          "Each plugin has a plugin.json manifest that defines its name, display name, entry point, and capabilities.",
          "Plugins appear automatically in the Plugin Store once installed. Enable them there to load them.",
          "The programsToLoad array in settings.json controls the load order of all providers, including plugins.",
        ],
      },
    ],
  },
  {
    key: "Development",
    children: [
      "Sicompass has an extensible plugin architecture. You can build your own providers in TypeScript or C to add new data sources and functionality.",
      "Plugins generate simple JSON arrays that Sicompass renders as navigable trees. This means any programming language that can output JSON to stdout can be used to build a plugin.",
      {
        key: "Creating a TypeScript Plugin",
        children: [
          "TypeScript plugins are the easiest way to extend Sicompass. They're simple scripts that receive a path as a command-line argument and output a JSON array to stdout.",
          "1. Create a folder: ~/.config/sicompass/plugins/my-plugin/",
          "2. Create a plugin.json manifest:",
          "   { \"name\": \"my-plugin\", \"displayName\": \"My Plugin\", \"entry\": \"plugin.ts\" }",
          "3. Write plugin.ts: read the path from process.argv[2], compute the children for that path, and output a JSON array to stdout.",
          "4. In the output JSON, strings become leaf items and objects become navigable sections. For example: [\"leaf item\", {\"Section Name\": [\"child 1\", \"child 2\"]}]",
          "5. Enable your plugin in Settings under 'Available programs'. It will appear in the Plugin Store automatically.",
          "Optional: add \"supportsConfigFiles\": true to plugin.json to enable Ctrl+S/O save/load functionality.",
          "See sdk/examples/typescript/ for a complete working example that you can use as a starting point.",
        ],
      },
      {
        key: "Creating a C Plugin",
        children: [
          "C plugins are compiled shared libraries that implement the ProviderOps interface. They offer maximum performance and full access to the Sicompass API.",
          "1. Create a folder: ~/.config/sicompass/plugins/my-c-plugin/",
          "2. Create a plugin.json manifest:",
          "   { \"name\": \"my-c-plugin\", \"displayName\": \"My C Plugin\", \"type\": \"native\", \"entry\": \"plugin.so\" }",
          "3. Write a C source file that includes <provider_interface.h> and exports: const ProviderOps* sicompass_plugin_init(void)",
          "4. Return a pointer to a static ProviderOps struct. At minimum, you must set the name, displayName, and fetch function pointers.",
          "5. Compile as a shared library: cc -shared -fPIC -o plugin.so plugin.c -I<path-to-sdk/include>",
          "6. Enable your plugin in Settings under 'Available programs'.",
          "C plugins can implement any subset of the ProviderOps functions. Only fetch is required. The more functions you implement, the richer the experience.",
          "See sdk/examples/c/ for a complete working example with build instructions.",
        ],
      },
      {
        key: "Provider Types",
        children: [
          "There are three ways to create a provider, each suited to different use cases:",
          "C Provider (ProviderOps): implement a ProviderOps struct and call providerCreate(ops). Best for high-performance providers that need direct memory access.",
          "Script Provider: write a TypeScript (or any language) script that outputs JSON. Loaded via scriptProviderCreate(name, displayName, scriptPath). Best for rapid development and prototyping.",
          "Factory Provider: register a creation function with providerFactoryRegister(name, createFn), then instantiate providers by name. Best for providers that need dynamic instantiation.",
        ],
      },
      {
        key: "ProviderOps Functions",
        children: [
          "The ProviderOps struct defines the full set of functions a provider can implement. Only 'fetch' is required, all others are optional.",
          {
            key: "Data",
            children: [
              "fetch(path): return an array of FFON elements for the given path. This is the only required function. It defines what content your provider shows.",
              "commitEdit(path, newValue): save an inline edit. Called when the user edits an <input> element and presses Enter. For example, renaming a file or changing a setting value.",
              "dashboardImagePath(path): return a path to an image that will be shown fullscreen when the user presses 'd'. Used by the Sales Demo for technical diagrams.",
              "supportsConfigFiles: when true, enables Ctrl+S/Shift+S/O for save/load. Set this in plugin.json: \"supportsConfigFiles\": true.",
            ],
          },
          {
            key: "Lifecycle",
            children: [
              "init(): called once at startup before any other operations. Use this to initialize state, open connections, or load cached data.",
              "cleanup(): called at shutdown to free resources. Close file handles, save state, and release memory here.",
              "loadConfig(filePath): load persistent configuration from the given file path. Called when the user presses Ctrl+O.",
              "saveConfig(filePath): save persistent configuration to the given file path. Called when the user presses Ctrl+S.",
            ],
          },
          {
            key: "Navigation",
            children: [
              "pushPath(segment): append a segment to the current path. Called when the user presses Right to go deeper into the tree.",
              "popPath(): remove the last segment from the current path. Called when the user presses Left to go back up.",
              "getCurrentPath(): return the current path as a string. Used by the app to display the current location.",
              "setCurrentPath(path): jump directly to an absolute path. Used after extended search to teleport the user to a result deep in the tree.",
            ],
          },
          {
            key: "File Operations",
            children: [
              "createDirectory(path, name): create a new directory at the given path. Triggered by the ':create directory' command.",
              "createFile(path, name): create a new file at the given path. Triggered by the ':create file' command.",
              "deleteItem(path): delete a file or directory at the given path. Directories are deleted recursively if non-empty.",
              "copyItem(source, destination): copy a file or directory from source to destination. Used by Ctrl+C/Ctrl+V.",
            ],
          },
          {
            key: "Commands",
            children: [
              "getCommands(): return a list of command names this provider supports. These appear when the user presses : (colon).",
              "handleCommand(name): prepare or validate a command. Optionally return a UI element (like a text input) for gathering additional input from the user.",
              "getCommandListItems(name): return a list of selectable options for a command. Shown as a navigable list the user can pick from.",
              "executeCommand(name, option): execute the command with the user's selected option. This is where the actual work happens.",
            ],
          },
          {
            key: "Events",
            children: [
              "onRadioChange(groupKey, selectedValue): called when the user changes a radio group selection. Use this to react to configuration changes in real time.",
              "onButtonPress(functionName): called when the user activates a <button> element. The functionName matches the value in the button tag.",
              "createElement(parentPath, templateKey): create a new FFON element for 'Add element:' sections (<one-opt> and <many-opt> elements).",
            ],
          },
          {
            key: "Search",
            children: [
              "collectDeepSearchItems(): return all searchable items for extended search (Ctrl+F). This lets you provide a custom index of searchable content.",
              "If not implemented, the system falls back to traversing the FFON tree automatically, which works well for most providers.",
            ],
          },
        ],
      },
      {
        key: "Element Tags",
        children: [
          "Element tags are special markers in string content that tell Sicompass to render interactive elements instead of plain text.",
          "Use \\\\< and \\\\> to escape angle brackets when you want to display them as literal text.",
          "\\<input>content\\</input> - make the content editable inline. The user can press i or a to edit it.",
          "\\<radio>group name - mark a parent object as a radio group. Its children become mutually exclusive options.",
          "\\<checked>option - mark a radio option as the currently selected one.",
          "\\<checkbox>label - render an unchecked boolean toggle. Press Enter to check it.",
          "\\<checkbox checked>label - render a checked boolean toggle. Press Enter to uncheck it.",
          "\\<link>path/to/file.json\\</link> - lazy-load an external JSON or FFON file as children when the user navigates into this item.",
          "\\<image>path/to/image.jpg\\</image> - display an image inline within the tree.",
          "\\<button>functionName\\</button>Display Text - render a clickable button. When activated, calls onButtonPress with the function name.",
          "\\<many-opt>\\</many-opt>key - a repeatable creation button. The user can add multiple instances. Each instance can be deleted later.",
          "\\<one-opt>\\</one-opt>key - a single-use creation button. After creation, the button is replaced by the created element.",
          "All tags support prefix and suffix text: 'Label: \\<input>value\\</input> (hint)' renders 'Label: ' before and ' (hint)' after the interactive element.",
          "This works for input, link, image, and button tags.",
          "All elements support \\\\n for multiline content. Continuation lines automatically inherit the prefix formatting.",
        ],
      },
    ],
  },
  {
    key: "Next Steps",
    children: [
      "Sicompass is actively growing. Here is what's on the roadmap:",
      "Notebook - structured note-taking with server-side sync, turning your notes into a navigable tree.",
      "IDE - code as a navigable structure. Browse functions, classes, and modules as a tree with C code generation.",
      "Terminal - a terminal emulator integrated as a provider, so you never need to leave Sicompass.",
      "Blog - publish structured content with optional paid access, viewable in both Sicompass and web browsers.",
      "Mobile - Android and iOS versions, bringing the same keyboard-driven (and touch-adapted) experience to mobile devices.",
      "Contributions are welcome! Whether it's code, plugins, documentation, or feedback, every contribution helps make computing more accessible.",
      "Join the community on Discord to connect with other users and developers.",
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
