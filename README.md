# Silicon's Compass

**Accessibility done better** — A keyboard-first document editor and navigation system.

## What is Sicompass?

Sicompass is an open-source, Vulkan-based application designed from the ground up with accessibility as its core principle. It bridges the gap between visual and non-visual interfaces, providing a fast, keyboard-navigable experience for structured data.

Built for users who prefer or require keyboard-driven interaction, Sicompass offers intuitive keyboard navigation and seamless screen reader integration through AccessKit.

## Key Features

- **Keyboard-First Design** — Navigate entirely with your keyboard
- **Native Accessibility** — Built-in support for screen readers on Linux, macOS, and Windows
- **Cross-Platform** — Runs on Linux, macOS, and Windows (x86_64 and ARM)
- **High-Performance Rendering** — Vulkan-powered graphics with FreeType2/HarfBuzz text shaping
- **Extensible Architecture** — Provider-based plugin system for different data sources

## Usage

Navigate Sicompass using simple keyboard controls:

- **Up/Down Arrows** — Navigate through list items
- **Left/Right Arrows** — Move between hierarchical layers
- **Enter** — Select or confirm
- **Tab** — Move between layers
- **Escape** — Go back or cancel

For power users, vim-style navigation (HJKL) is also supported.

## Getting Started

Download the latest release for your platform from the [GitHub Releases](https://github.com/nicoverrijdt/sicompass/releases) page.

### Building from Source

```bash
# Optional: use Nix for dependency management
nix develop

meson setup build
ninja -C build
```

## The Vision

Sicompass is the foundation for a unified, accessible platform. Future development includes:

- **File Browser** — Navigate your filesystem with keyboard commands
- **Web Browser** — Accessible web browsing experience
- **Email & Chat** — Integrated communication tools
- **Notebook** — Structured note-taking
- **Mobile Support** — Android and iOS versions

The goal is to provide a comprehensive environment where functionality and accessibility come first — an alternative approach that makes computing more accessible to everyone.

## Community

Join the conversation on [Discord](https://discord.com/channels/1464152138753249313/1464152139231137894).

## License

MIT License — See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Whether it's code, documentation, or feedback, your input helps make computing more accessible for everyone

