use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use std::path::Path;

// ---------------------------------------------------------------------------
// Tutorial content tree
// ---------------------------------------------------------------------------

/// A node in the tutorial content tree (mirrors the TypeScript `Section` type).
enum Node {
    Leaf(&'static str),
    Branch { key: &'static str, children: &'static [Node] },
}

use Node::{Branch, Leaf};

fn lorem_ipsum() -> &'static str {
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium."
}

// Static tree matching tutorial.ts `sections` array.
// Asset paths (TEXTURE_JPG, SF_JSON) are substituted at runtime via TutorialProvider.

static SECTIONS: &[Node] = &[
    Branch {
        key: "Welcome --> here you can go up, down, right or left",
        children: &[
            Leaf("Welcome to Sicompass, a keyboard-driven interface for browsing and managing structured data."),
            Leaf("Every graphical interface is essentially a tree of lists. Sicompass makes that structure explicit and navigable entirely from the keyboard, eliminating the need for a mouse."),
            Leaf("This means one consistent way to navigate everything: files, documents, settings, web pages, and any data source built on top of it."),
            Leaf("Each top-level item you see at the root is a program (also called a provider). Providers plug into the same unified interface, so once you learn to navigate one, you can navigate them all."),
            Leaf("This tutorial is itself a provider. Everything you learn here, you're practicing right now by navigating through it."),
            Leaf("Use the Right arrow key to dive into any section, and Left to come back. Let's get started!"),
        ],
    },
    Branch {
        key: "Navigation",
        children: &[
            Leaf("Navigation in Sicompass works like a file manager: you move through a tree of items using the arrow keys. Every provider, whether it's a file browser, web browser, or this tutorial, uses the same navigation model."),
            Branch {
                key: "Moving Around",
                children: &[
                    Leaf("Up key: move the selection up in the current list"),
                    Leaf("Down key: move the selection down in the current list"),
                    Leaf("Right key: go into the selected item (descend into its children)"),
                    Leaf("Left key: go back to the parent level (ascend one level up)"),
                    Leaf("Enter: confirm or activate the selected item (e.g. toggle a checkbox, open a link)"),
                    Leaf("These five keys are all you need to navigate any content in Sicompass. The tree can be arbitrarily deep. Just keep pressing Right to go deeper, and Left to come back."),
                ],
            },
            Branch {
                key: "Modes",
                children: &[
                    Leaf("Sicompass has several modes that change what your keyboard inputs do. You always start in operator mode."),
                    Leaf("Space: toggle between operator mode and editor mode. Operator mode is for navigating; editor mode enables additional editing shortcuts."),
                    Leaf(": (colon): enter command mode. Type a command name and press Enter to execute it. Commands are context-sensitive, each provider can offer its own commands."),
                    Leaf("Tab: enter simple search mode. Start typing to filter items in the current list. Only items matching your search will be shown."),
                    Leaf("Tab again (from search mode): switch to scroll mode. Use Up/Down to scroll through a long text body without moving the selection."),
                    Leaf("Ctrl+F: enter extended search mode. This searches recursively through all children, not just the current level. Results are shown as a flat list you can jump to."),
                    Leaf("Escape: go back to the previous mode, or cancel the current operation."),
                ],
            },
        ],
    },
    Branch {
        key: "Accessibility",
        children: &[
            Leaf("Sicompass has built-in screen reader support powered by AccessKit. If you use a screen reader, Sicompass works with it out of the box — no configuration needed."),
            Leaf("Screen reader support is available on all platforms: Linux (AT-SPI), macOS (VoiceOver), and Windows (Narrator, NVDA, JAWS)."),
            Leaf("When you navigate up, down, or into items, the current element is automatically announced by your screen reader."),
            Leaf("Mode changes are also announced. For example, switching to insert mode announces 'editor insert', entering search announces 'search', and returning to normal navigation announces 'operator mode'."),
            Leaf("Screen reader support activates automatically when a screen reader is detected. There is nothing to enable or configure."),
        ],
    },
    Branch {
        key: "Editing",
        children: &[
            Leaf("Some items in Sicompass are editable. You can tell because they contain an <input> tag. For example, file names in the file browser or setting values can be edited inline."),
            Leaf("Press i to enter insert mode. Your cursor is placed at the beginning of the editable text, and you can type to replace or modify it."),
            Leaf("Press a to enter append mode. Your cursor is placed at the end of the editable text, so you can add to what's already there."),
            Leaf("While editing, type normally to change the text. Use Backspace to delete characters."),
            Leaf("Press Enter to confirm your edit and save the change."),
            Leaf("Press Escape to cancel the edit and discard your changes."),
            Leaf("Not all items are editable, only those marked with <input> tags by the provider. The file browser makes file and directory names editable; the settings provider makes configuration values editable."),
        ],
    },
    Branch {
        key: "Commands",
        children: &[
            Leaf("Commands let you perform actions beyond simple navigation and editing. Each provider can define its own set of commands."),
            Leaf("Press : (colon) to enter command mode. A command prompt appears at the bottom of the screen."),
            Leaf("Start typing the command name. Matching commands will appear as suggestions. Press Enter to select one."),
            Leaf("Some commands take additional input. For example, 'create file' in the file browser will prompt you for a filename."),
            Leaf("Common file browser commands:"),
            Leaf(":create file - create a new file in the current directory"),
            Leaf(":create directory - create a new directory in the current directory"),
            Leaf("Providers can define any commands they want. Check each provider's command list by pressing : and browsing the suggestions."),
        ],
    },
    Branch {
        key: "Programs",
        children: &[
            Leaf("Programs (also called providers) are the building blocks of Sicompass. Each program turns a different data source into the same navigable tree structure."),
            Leaf("Programs appear as top-level items when you navigate to the root (press Left until you can't go further)."),
            Leaf("You can configure which programs are loaded in Settings under 'Available programs'. Enable or disable them at any time, changes take effect instantly."),
            Branch { key: "File Browser", children: &[
                Leaf("The file browser turns your filesystem into a navigable tree. Directories become sections you can enter with the Right key; files are leaf items."),
                Leaf("Browse your filesystem by navigating up, down, and into directories. The current path is shown at the top of the screen."),
                Leaf("Rename files and directories by pressing i (insert mode) on any item. The name becomes editable inline. Type the new name and press Enter to confirm."),
                Leaf("Create new items inline: press Ctrl+I or Ctrl+A to enter insert mode on an empty placeholder. Type a plain name to create a file, or append : to the name to create a directory (the colon is stripped from the directory name). For example, type 'notes' to create a file, or 'projects:' to create a directory named 'projects'."),
                Leaf("You can also create items via commands: press : and type 'create file' or 'create directory'."),
                Leaf("Delete items by pressing the Delete key on a selected item. Directories are deleted recursively if non-empty."),
                Leaf("Copy with Ctrl+C and paste with Ctrl+V to duplicate files and directories."),
                Leaf("The file browser supports all the standard modes: search (Tab) to filter files, extended search (Ctrl+F) to find files recursively in subdirectories."),
            ]},
            Branch { key: "Sales Demo", children: &[
                Leaf("The Sales Demo is an interactive air handling unit (HVAC) product configurator. It demonstrates how Sicompass can handle complex, hierarchical data with inline editing."),
                Leaf("Navigate supply air and return air sections to explore components like filters, coils, fans, and recovery wheels."),
                Leaf("Each component has editable parameters (temperatures, pressures, dimensions, and more) that you can modify inline."),
                Leaf("Optional components (chillers, fan coil units) can be added via 'Add element:' sections. These use the <one-opt> and <many-opt> element types."),
                Leaf("Press 'd' at the root level to view a technical unit diagram as a fullscreen image. This showcases the dashboardImagePath provider feature."),
                Leaf("This provider is a good reference for building data-heavy configurators on top of Sicompass."),
            ]},
            Branch { key: "Web Browser", children: &[
                Leaf("The web browser lets you browse the internet directly inside Sicompass, turning web pages into keyboard-navigable trees."),
                Leaf("At the top level, you'll find an address bar. Press i to edit it, type a URL, and press Enter to load the page."),
                Leaf("HTML is automatically converted into a navigable FFON tree based on the page's heading hierarchy (h1-h6). Headings become nested sections; paragraphs, lists, tables, and links are preserved as tree items."),
                Leaf("Navigate web content the same way you navigate files or settings. Right to go deeper into a section, Left to go back."),
                Leaf("Links on web pages can be followed by selecting them and pressing Enter, which loads the linked page."),
                Leaf("This demonstrates how any structured content, even the web, can be unified into the same navigation model."),
            ]},
            Branch { key: "Plugin Store", children: &[
                Leaf("The Plugin Store lets you manage which providers are active. It appears in Settings under 'Available programs'."),
                Leaf("Each provider is shown as a checkbox. Check it to enable the provider, uncheck it to disable it."),
                Leaf("Changes take effect immediately. Providers are hot-loaded or unloaded without restarting the app."),
                Leaf("Both built-in providers and user-installed plugins appear here. Plugins installed in ~/.config/sicompass/plugins/ are automatically discovered."),
                Leaf("This is the easiest way to customize your Sicompass setup. Enable only the programs you use."),
            ]},
            Branch { key: "Remote Services", children: &[
                Leaf("Sicompass can connect to remote FFON providers served over HTTP, extending the interface beyond your local machine."),
                Leaf("Configure a remoteUrl and optional apiKey in Settings to connect to a remote service."),
                Leaf("Remote content is lazily fetched as you navigate. Only the data you actually view is downloaded, keeping things fast even with large datasets."),
                Leaf("Providers can use the included TypeScript server SDK to build FFON services with optional Stripe or LemonSqueezy billing integration."),
                Leaf("This enables SaaS-style products where the entire user interface is delivered through Sicompass's navigable tree."),
            ]},
            Branch { key: "Chat Client (not yet functional)", children: &[
                Leaf("A planned Matrix protocol chat client for real-time messaging inside Sicompass."),
                Leaf("Rooms and messages will appear as a navigable tree. Rooms as sections, messages as items within them."),
                Leaf("Send messages by editing inline within a room, using the same i/a editing keys."),
                Leaf("Configure homeserver URL and credentials in Settings."),
                Leaf("This provider is under development and not yet functional."),
            ]},
            Branch { key: "Email Client (not yet functional)", children: &[
                Leaf("A planned email client supporting IMAP for reading and SMTP for sending."),
                Leaf("Supports Google OAuth2 for seamless Gmail account integration."),
                Leaf("Folders (inbox, sent, drafts) and messages will appear as a navigable tree, with message bodies rendered as readable text items."),
                Leaf("Configure server URLs and credentials in Settings."),
                Leaf("This provider is under development and not yet functional."),
            ]},
            Branch { key: "Settings", children: &[
                Leaf("The settings provider is always loaded as the last item in the root. It's where you configure Sicompass itself and all loaded providers."),
                Leaf("Settings are organized by namespace. 'sicompass' for global settings, and each provider can have its own section."),
                Leaf("Color scheme (dark/light) is configured here as a radio group. Changes take effect immediately."),
                Leaf("The 'Available programs' section is where you enable and disable providers (see Plugin Store above)."),
                Leaf("All settings are stored in ~/.config/sicompass/settings.json. You can edit this file directly if you prefer."),
            ]},
        ],
    },
    // "Interactive Elements" section — leaf nodes with tag examples.
    // Asset paths (<image> and <link>) are filled in at runtime by TutorialProvider.
    Branch {
        key: "Interactive Elements",
        children: &[
            Leaf("This section is a hands-on playground for all the interactive element types that Sicompass supports. Try each one as you go!"),
            Leaf("Checkboxes are boolean toggles. Press Enter on a checkbox to toggle it on or off."),
            Leaf("<checkbox checked>Try toggling this checkbox (it starts checked)"),
            Leaf("<checkbox>And this unchecked one"),
            Branch { key: "<checkbox checked>Navigable checkbox (go inside with Right key)", children: &[
                Leaf("This is an object checkbox. It can be toggled AND navigated into."),
                Leaf("Press Enter to toggle the checkbox state, or press the Right key to view these children."),
                Leaf("Object checkboxes are useful when you want a feature toggle that also has sub-settings. For example, enabling a provider while also configuring its options."),
            ]},
            Branch { key: "<checkbox>Another navigable checkbox (unchecked)", children: &[
                Leaf("Object checkboxes work the same whether checked or unchecked. The checkbox state and the children are independent."),
            ]},
            Leaf("Text inputs let you edit a value inline. Press i or a on the item below to start editing:"),
            Leaf("Edit this text --> <input>hello world</input> <-- press i or a"),
            Branch { key: "<radio>Pick a color", children: &[
                Leaf("<checked>blue"),
                Leaf("green"),
                Leaf("red"),
            ]},
            Leaf("Radio groups let you pick exactly one option from a set. Navigate into the radio group above and press Enter on an option to select it. Only one option can be selected at a time."),
            Leaf("Images can be displayed inline within the tree. The image below is loaded from a file path:"),
            // TEXTURE_JPG placeholder — replaced by TutorialProvider::make_interactive_elements
            Leaf("__TEXTURE_JPG__"),
            Leaf("__IMAGE_WITH_PREFIX_SUFFIX__"),
            Leaf("__IMAGE_SUFFIX_ONLY__"),
            Leaf("__IMAGE_PREFIX_ONLY__"),
            Leaf("Links lazy-load external JSON or FFON files as children. Navigate into the link below to load its content:"),
            // SF_JSON placeholder — replaced at runtime
            Branch { key: "__LINK_WITH_PREFIX_SUFFIX__", children: &[] },
            Leaf("Scroll mode: when a text item is too long to fit on screen, you can scroll through it. Press Tab twice from operator mode to enter scroll mode, then use Up/Down to scroll the text below:"),
            Leaf("__LOREM_IPSUM__"),
        ],
    },
    Branch {
        key: "Configuration",
        children: &[
            Leaf("Sicompass stores all configuration in a single file: ~/.config/sicompass/settings.json. This file is organized by namespace, each provider can have its own section."),
            Branch { key: "Save and Load", children: &[
                Leaf("Some providers support saving and loading configuration files. This is useful for product configurators or any provider that manages persistent state."),
                Leaf("Ctrl+S: save the active provider's data to its default config file."),
                Leaf("Ctrl+Shift+S: save as, choose a custom filename for the saved configuration."),
                Leaf("Ctrl+O: open/load a saved configuration file."),
                Leaf("These shortcuts only work if the active provider has enabled config file support. Plugins enable this by adding \"supportsConfigFiles\": true to their plugin.json manifest."),
            ]},
            Branch { key: "Settings File", children: &[
                Leaf("The main settings file at ~/.config/sicompass/settings.json uses a namespaced JSON format."),
                Leaf("Example structure: { \"sicompass\": { \"colorScheme\": \"dark\", \"programsToLoad\": [...] }, \"file browser\": { \"sortOrder\": \"name\" } }"),
                Leaf("The \"sicompass\" namespace contains global settings like color scheme and which programs to load."),
                Leaf("Each provider adds its own namespace for provider-specific settings."),
                Leaf("You can edit this file directly, but changes require a restart to take effect. Using the Settings provider inside Sicompass applies changes immediately."),
            ]},
            Branch { key: "Plugin Configuration", children: &[
                Leaf("User plugins are installed in ~/.config/sicompass/plugins/<plugin-name>/."),
                Leaf("Each plugin has a plugin.json manifest that defines its name, display name, entry point, and capabilities."),
                Leaf("Plugins appear automatically in the Plugin Store once installed. Enable them there to load them."),
                Leaf("The programsToLoad array in settings.json controls the load order of all providers, including plugins."),
            ]},
        ],
    },
    Branch {
        key: "Development",
        children: &[
            Leaf("Sicompass has an extensible plugin architecture. You can build your own providers in TypeScript or C to add new data sources and functionality."),
            Leaf("Plugins generate simple JSON arrays that Sicompass renders as navigable trees. This means any programming language that can output JSON to stdout can be used to build a plugin."),
            Branch { key: "Creating a TypeScript Plugin", children: &[
                Leaf("TypeScript plugins are the easiest way to extend Sicompass. They're simple scripts that receive a path as a command-line argument and output a JSON array to stdout."),
                Leaf("1. Create a folder: ~/.config/sicompass/plugins/my-plugin/"),
                Leaf("2. Create a plugin.json manifest:"),
                Leaf("   { \"name\": \"my-plugin\", \"displayName\": \"My Plugin\", \"entry\": \"plugin.ts\" }"),
                Leaf("3. Write plugin.ts: read the path from process.argv[2], compute the children for that path, and output a JSON array to stdout."),
                Leaf("4. In the output JSON, strings become leaf items and objects become navigable sections. For example: [\"leaf item\", {\"Section Name\": [\"child 1\", \"child 2\"]}]"),
                Leaf("5. Enable your plugin in Settings under 'Available programs'. It will appear in the Plugin Store automatically."),
                Leaf("Optional: add \"supportsConfigFiles\": true to plugin.json to enable Ctrl+S/O save/load functionality."),
                Leaf("See sdk/examples/typescript/ for a complete working example that you can use as a starting point."),
            ]},
            Branch { key: "Creating a C Plugin", children: &[
                Leaf("C plugins are compiled shared libraries that implement the ProviderOps interface. They offer maximum performance and full access to the Sicompass API."),
                Leaf("1. Create a folder: ~/.config/sicompass/plugins/my-c-plugin/"),
                Leaf("2. Create a plugin.json manifest:"),
                Leaf("   { \"name\": \"my-c-plugin\", \"displayName\": \"My C Plugin\", \"type\": \"native\", \"entry\": \"plugin.so\" }"),
                Leaf("3. Write a C source file that includes <provider_interface.h> and exports: const ProviderOps* sicompass_plugin_init(void)"),
                Leaf("4. Return a pointer to a static ProviderOps struct. At minimum, you must set the name, displayName, and fetch function pointers."),
                Leaf("5. Compile as a shared library: cc -shared -fPIC -o plugin.so plugin.c -I<path-to-sdk/include>"),
                Leaf("6. Enable your plugin in Settings under 'Available programs'."),
                Leaf("C plugins can implement any subset of the ProviderOps functions. Only fetch is required. The more functions you implement, the richer the experience."),
                Leaf("See sdk/examples/c/ for a complete working example with build instructions."),
            ]},
            Branch { key: "Provider Types", children: &[
                Leaf("There are three ways to create a provider, each suited to different use cases:"),
                Leaf("C Provider (ProviderOps): implement a ProviderOps struct and call providerCreate(ops). Best for high-performance providers that need direct memory access."),
                Leaf("Script Provider: write a TypeScript (or any language) script that outputs JSON. Loaded via scriptProviderCreate(name, displayName, scriptPath). Best for rapid development and prototyping."),
                Leaf("Factory Provider: register a creation function with providerFactoryRegister(name, createFn), then instantiate providers by name. Best for providers that need dynamic instantiation."),
            ]},
            Branch { key: "ProviderOps Functions", children: &[
                Leaf("The ProviderOps struct defines the full set of functions a provider can implement. Only 'fetch' is required, all others are optional."),
                Branch { key: "Data", children: &[
                    Leaf("fetch(path): return an array of FFON elements for the given path. This is the only required function. It defines what content your provider shows."),
                    Leaf("commitEdit(path, newValue): save an inline edit. Called when the user edits an <input> element and presses Enter. For example, renaming a file or changing a setting value."),
                    Leaf("dashboardImagePath(path): return a path to an image that will be shown fullscreen when the user presses 'd'. Used by the Sales Demo for technical diagrams."),
                    Leaf("supportsConfigFiles: when true, enables Ctrl+S/Shift+S/O for save/load. Set this in plugin.json: \"supportsConfigFiles\": true."),
                ]},
                Branch { key: "Lifecycle", children: &[
                    Leaf("init(): called once at startup before any other operations. Use this to initialize state, open connections, or load cached data."),
                    Leaf("cleanup(): called at shutdown to free resources. Close file handles, save state, and release memory here."),
                    Leaf("loadConfig(filePath): load persistent configuration from the given file path. Called when the user presses Ctrl+O."),
                    Leaf("saveConfig(filePath): save persistent configuration to the given file path. Called when the user presses Ctrl+S."),
                ]},
                Branch { key: "Navigation", children: &[
                    Leaf("pushPath(segment): append a segment to the current path. Called when the user presses Right to go deeper into the tree."),
                    Leaf("popPath(): remove the last segment from the current path. Called when the user presses Left to go back up."),
                    Leaf("getCurrentPath(): return the current path as a string. Used by the app to display the current location."),
                    Leaf("setCurrentPath(path): jump directly to an absolute path. Used after extended search to teleport the user to a result deep in the tree."),
                ]},
                Branch { key: "File Operations", children: &[
                    Leaf("createDirectory(path, name): create a new directory at the given path. Triggered by the ':create directory' command."),
                    Leaf("createFile(path, name): create a new file at the given path. Triggered by the ':create file' command."),
                    Leaf("deleteItem(path): delete a file or directory at the given path. Directories are deleted recursively if non-empty."),
                    Leaf("copyItem(source, destination): copy a file or directory from source to destination. Used by Ctrl+C/Ctrl+V."),
                ]},
                Branch { key: "Commands", children: &[
                    Leaf("getCommands(): return a list of command names this provider supports. These appear when the user presses : (colon)."),
                    Leaf("handleCommand(name): prepare or validate a command. Optionally return a UI element (like a text input) for gathering additional input from the user."),
                    Leaf("getCommandListItems(name): return a list of selectable options for a command. Shown as a navigable list the user can pick from."),
                    Leaf("executeCommand(name, option): execute the command with the user's selected option. This is where the actual work happens."),
                ]},
                Branch { key: "Events", children: &[
                    Leaf("onRadioChange(groupKey, selectedValue): called when the user changes a radio group selection. Use this to react to configuration changes in real time."),
                    Leaf("onButtonPress(functionName): called when the user activates a <button> element. The functionName matches the value in the button tag."),
                    Leaf("createElement(parentPath, templateKey): create a new FFON element for 'Add element:' sections (<one-opt> and <many-opt> elements)."),
                ]},
                Branch { key: "Search", children: &[
                    Leaf("collectDeepSearchItems(): return all searchable items for extended search (Ctrl+F). This lets you provide a custom index of searchable content."),
                    Leaf("If not implemented, the system falls back to traversing the FFON tree automatically, which works well for most providers."),
                ]},
            ]},
            Branch { key: "Element Tags", children: &[
                Leaf("Element tags are special markers in string content that tell Sicompass to render interactive elements instead of plain text."),
                Leaf("Use \\< and \\> to escape angle brackets when you want to display them as literal text."),
                Leaf("\\<input>content\\</input> - make the content editable inline. The user can press i or a to edit it."),
                Leaf("\\<radio>group name - mark a parent object as a radio group. Its children become mutually exclusive options."),
                Leaf("\\<checked>option - mark a radio option as the currently selected one."),
                Leaf("\\<checkbox>label - render an unchecked boolean toggle. Press Enter to check it."),
                Leaf("\\<checkbox checked>label - render a checked boolean toggle. Press Enter to uncheck it."),
                Leaf("\\<link>path/to/file.json\\</link> - lazy-load an external JSON or FFON file as children when the user navigates into this item."),
                Leaf("\\<image>path/to/image.jpg\\</image> - display an image inline within the tree."),
                Leaf("\\<button>functionName\\</button>Display Text - render a clickable button. When activated, calls onButtonPress with the function name."),
                Leaf("\\<many-opt>\\</many-opt>key - a repeatable creation button. The user can add multiple instances. Each instance can be deleted later."),
                Leaf("\\<one-opt>\\</one-opt>key - a single-use creation button. After creation, the button is replaced by the created element."),
                Leaf("All tags support prefix and suffix text: 'Label: \\<input>value\\</input> (hint)' renders 'Label: ' before and ' (hint)' after the interactive element."),
                Leaf("This works for input, link, image, and button tags."),
                Leaf("All elements support \\\\n for multiline content. Continuation lines automatically inherit the prefix formatting."),
            ]},
        ],
    },
    Branch {
        key: "Next Steps",
        children: &[
            Leaf("Sicompass is actively growing. Here is what's on the roadmap:"),
            Leaf("Notebook - structured note-taking with server-side sync, turning your notes into a navigable tree."),
            Leaf("IDE - code as a navigable structure. Browse functions, classes, and modules as a tree with C code generation."),
            Leaf("Terminal - a terminal emulator integrated as a provider, so you never need to leave Sicompass."),
            Leaf("Blog - publish structured content with optional paid access, viewable in both Sicompass and web browsers."),
            Leaf("Mobile - Android and iOS versions, bringing the same keyboard-driven (and touch-adapted) experience to mobile devices."),
            Leaf("Contributions are welcome! Whether it's code, plugins, documentation, or feedback, every contribution helps make computing more accessible."),
            Leaf("Join the community on Discord to connect with other users and developers."),
            Leaf("Happy navigating!"),
        ],
    },
];

// ---------------------------------------------------------------------------
// Navigation helper — mirrors getChildrenAtPath in tutorial.ts
// ---------------------------------------------------------------------------

/// Returns the children slice for `path_parts` within `nodes`, or `None` if not found.
fn get_children_at_path<'a>(
    nodes: &'a [Node],
    path_parts: &[&str],
) -> Option<&'a [Node]> {
    if path_parts.is_empty() {
        return Some(nodes);
    }
    let (head, rest) = (&path_parts[0], &path_parts[1..]);
    for node in nodes {
        if let Node::Branch { key, children } = node {
            if *key == *head {
                return get_children_at_path(children, rest);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Convert static tree to FfonElement vec, substituting asset paths
// ---------------------------------------------------------------------------

fn node_to_ffon(node: &Node, texture_jpg: &str, ffon_json: &str) -> FfonElement {
    match node {
        Node::Leaf(s) => FfonElement::Str(apply_asset_placeholders(s, texture_jpg, ffon_json)),
        Node::Branch { key, children } => {
            let resolved_key = apply_asset_placeholders(key, texture_jpg, ffon_json);
            let mut obj = FfonElement::new_obj(resolved_key);
            for child in *children {
                obj.as_obj_mut()
                    .unwrap()
                    .push(node_to_ffon(child, texture_jpg, ffon_json));
            }
            obj
        }
    }
}

fn apply_asset_placeholders(s: &str, texture_jpg: &str, ffon_json: &str) -> String {
    match s {
        "__TEXTURE_JPG__" => format!("<image>{texture_jpg}</image>"),
        "__IMAGE_WITH_PREFIX_SUFFIX__" => {
            format!("Image with prefix: <image>{texture_jpg}</image>and suffix")
        }
        "__IMAGE_SUFFIX_ONLY__" => format!("<image>{texture_jpg}</image>and suffix"),
        "__IMAGE_PREFIX_ONLY__" => format!("Image with prefix: <image>{texture_jpg}</image>"),
        "__LINK_WITH_PREFIX_SUFFIX__" => {
            format!("Playground based on html <link>{ffon_json}</link> which can be with prefix and suffix")
        }
        "__LOREM_IPSUM__" => lorem_ipsum().to_owned(),
        other => other.to_owned(),
    }
}

fn nodes_to_ffon(nodes: &[Node], texture_jpg: &str, ffon_json: &str) -> Vec<FfonElement> {
    nodes.iter().map(|n| node_to_ffon(n, texture_jpg, ffon_json)).collect()
}

// ---------------------------------------------------------------------------
// TutorialProvider
// ---------------------------------------------------------------------------

/// The tutorial provider: a read-only navigable guide to Sicompass.
///
/// `assets_dir` should point to the directory containing `texture.jpg` and `sf.json`.
/// These are the same assets used by the TypeScript tutorial (`lib/lib_tutorial/assets/`).
pub struct TutorialProvider {
    current_path: String,
    texture_jpg: String,
    ffon_json: String,
}

impl TutorialProvider {
    /// Create with explicit asset directory.
    pub fn new(assets_dir: &Path) -> Self {
        let texture_jpg = assets_dir.join("texture.jpg").to_string_lossy().replace('\\', "/");
        let ffon_json = assets_dir.join("ffon.json").to_string_lossy().replace('\\', "/");
        TutorialProvider {
            current_path: "/".to_owned(),
            texture_jpg,
            ffon_json,
        }
    }

    /// Convenience: create with an empty asset path (for tests that don't need images/links).
    pub fn new_headless() -> Self {
        TutorialProvider {
            current_path: "/".to_owned(),
            texture_jpg: "/missing/texture.jpg".to_owned(),
            ffon_json: "/missing/ffon.json".to_owned(),
        }
    }

    fn path_parts(&self) -> Vec<&str> {
        if self.current_path == "/" {
            vec![]
        } else {
            self.current_path.split('/').filter(|s| !s.is_empty()).collect()
        }
    }
}

impl Provider for TutorialProvider {
    fn name(&self) -> &str { "tutorial" }

    fn display_name(&self) -> &str {
        "tutorial --> here you can go up, down or right"
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let parts = self.path_parts();
        match get_children_at_path(SECTIONS, &parts) {
            Some(nodes) => nodes_to_ffon(nodes, &self.texture_jpg, &self.ffon_json),
            None => vec![],
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path.push('/');
            self.current_path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_tutorial/tutorial.test.ts (12 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> TutorialProvider {
        TutorialProvider::new_headless()
    }

    // Root level

    #[test]
    fn test_root_returns_top_level_sections() {
        let mut p = provider();
        let elems = p.fetch();
        assert!(!elems.is_empty());
        // First section is "Welcome..."
        let first = elems[0].as_obj().unwrap();
        assert!(first.key.starts_with("Welcome"));
    }

    #[test]
    fn test_root_contains_all_top_level_sections() {
        let mut p = provider();
        let elems = p.fetch();
        let keys: Vec<&str> = elems.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(keys.iter().any(|k| k.starts_with("Navigation")));
        assert!(keys.iter().any(|k| k.starts_with("Editing")));
        assert!(keys.iter().any(|k| k.starts_with("Commands")));
        assert!(keys.iter().any(|k| k.starts_with("Programs")));
        assert!(keys.iter().any(|k| k.starts_with("Interactive")));
        assert!(keys.iter().any(|k| k.starts_with("Development")));
        assert!(keys.iter().any(|k| k.starts_with("Next Steps")));
    }

    // Path navigation

    #[test]
    fn test_navigation_section_children() {
        let mut p = provider();
        p.push_path("Navigation");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        // First child is a string (overview text)
        assert!(elems[0].is_str());
        // Then "Moving Around" and "Modes" sections
        let section_keys: Vec<&str> = elems.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(section_keys.contains(&"Moving Around"));
        assert!(section_keys.contains(&"Modes"));
    }

    #[test]
    fn test_nested_navigation() {
        let mut p = provider();
        p.push_path("Navigation");
        p.push_path("Modes");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        assert!(elems[0].is_str());
    }

    #[test]
    fn test_unknown_path_returns_empty() {
        let mut p = provider();
        p.push_path("NonExistentSection");
        let elems = p.fetch();
        assert!(elems.is_empty());
    }

    // Interactive elements

    #[test]
    fn test_interactive_elements_contains_checkbox() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_checked_checkbox = elems.iter().any(|e| {
            e.as_str().map_or(false, |s| s.starts_with("<checkbox checked>"))
        });
        assert!(has_checked_checkbox);
    }

    #[test]
    fn test_interactive_elements_contains_radio_group() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_radio = elems.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.starts_with("<radio>"))
        });
        assert!(has_radio);
    }

    #[test]
    fn test_interactive_elements_contains_input() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_input = elems.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<input>"))
        });
        assert!(has_input);
    }

    #[test]
    fn test_interactive_elements_contains_image() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_image = elems.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<image>"))
        });
        assert!(has_image);
    }

    // Pop path

    #[test]
    fn test_pop_path_returns_to_root() {
        let mut p = provider();
        p.push_path("Navigation");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        let elems = p.fetch();
        // Should return all top-level sections again
        assert!(elems.len() > 5);
    }

    #[test]
    fn test_pop_path_from_nested() {
        let mut p = provider();
        p.push_path("Navigation");
        p.push_path("Modes");
        p.pop_path();
        assert_eq!(p.current_path(), "/Navigation");
        let elems = p.fetch();
        // Should return Navigation's children
        let keys: Vec<&str> = elems.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(keys.contains(&"Moving Around"));
    }

    // set_current_path

    #[test]
    fn test_set_current_path() {
        let mut p = provider();
        p.set_current_path("/Navigation/Modes");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        assert!(elems[0].is_str());
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the tutorial with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("tutorial", || {
        let assets = sicompass_sdk::platform::resolve_repo_asset("lib/lib_tutorial/assets");
        Box::new(TutorialProvider::new(&assets))
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("tutorial", "tutorial").enable_by_default(),
    );
}
