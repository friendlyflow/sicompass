# Silicon's Compass

*A keyboard-first, accessibility-first way to use your entire computer.*

Sicompass is a free, open-source way to use your computer entirely from the keyboard, with no mouse needed. Under the hood, every screen is just a tree of lists. Sicompass shows you that structure directly, so you move through it with the arrow keys.

You navigate everything the same way, your files, settings, email, and any other data. It is fast, precise, and predictable, it feels the same everywhere, and it works smoothly with screen readers on Linux, macOS, and Windows. It does not try to look pretty, and that is exactly the point.

## Download

Prebuilt packages are published on the [Releases page](../../releases/latest).

- **Windows**: download the latest packaged build from Releases
- **macOS** and **Linux**: build from source for now (packaged releases planned)

## Getting Started

Build from source:

```bash
# Optional: use Nix for dependency management
nix develop

# Build
cargo build --release

# Run
cargo run --release
```

## Key Features

- **Unambiguous Focus**: You always know where the focus is, no guessing
- **Flat Interface**: No popups, dialogs, or overlays, everything is navigated inline within the tree
- **Keyboard-First**: Your hands never leave the keyboard, with tabbed workspaces and letter-driven command palettes
- **Native Accessibility**: Built-in screen reader support on Linux, macOS, and Windows
- **Cross-Platform**: Tested on Ubuntu today, with paths, shells, and PTYs routed through platform helpers
- **High-Performance Rendering**: Vulkan graphics with a FreeType2 glyph atlas
- **Extensible**: Provider-based plugin system with a built-in store for hot enable/disable

## Built-in Providers

Each provider turns a different data source into the same keyboard-navigable tree: File Browser, Text Editor, Email, Chat, Web Browser, Terminal, Plugin Store, and Settings.

## Community

Join the conversation on [Discord](https://discord.com/channels/1464152138753249313/1464152139231137894).

## License

<!-- #### Commercial license

If you want to use Sicompass to develop commercial projects and applications, the Commercial license is the appropriate license. With this option, your source code is kept proprietary.
[Read more about the commercial license](https://sicompass.org/license/) -->

#### Open source license

If you are creating an open source application under a license compatible with
the GNU GPL license v3, you may use this project under the terms of the GPLv3.

## Contributing

Contributions are welcome! Whether it's code, documentation, or feedback, your input helps make computing more accessible for everyone
