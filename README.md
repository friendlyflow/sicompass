# Silicon's Compass

Another structure for people who like structure, but tangible.

## What is Sicompass?

Sicompass is an open-source, keyboard-only interface for navigating and editing structured content. Every graphical interface is essentially a tree of lists — Sicompass makes that structure explicit and navigable entirely from the keyboard, eliminating the need for a mouse.

This consistent unified interaction model means one consistent way to navigate everything: files, documents, settings, and any data source built on top of it. For developers, it means simpler UI development with fewer bugs. For users, it means a unified, fast, precise and handy experience that works the same everywhere.

<!-- Built for users who prefer or require keyboard-driven interaction, Sicompass offers intuitive keyboard navigation and seamless screen reader integration through AccessKit. -->

## Key Features

- **Keyboard-First Design** — Faster and handier navigation because your hands never leave the keyboard
<!-- - **Native Accessibility** — Built-in support for screen readers on Linux, macOS, and Windows -->
- **Cross-Platform** — Currently developed and tested on Ubuntu, with macOS and Windows releases planned for later
- **High-Performance Rendering** — Vulkan-powered graphics with FreeType2/HarfBuzz text shaping
- **Extensible Architecture** — Provider-based plugin system for different data sources
- **Simpler UI Development** — Functionality over design means less complexity and faster development with minimal styling

## Usage

Navigate Sicompass using simple keyboard controls:

- **Up/Down Arrows** — Navigate through list items
- **Left/Right Arrows** — Move between hierarchical layers
- **Enter** — Select/Right or confirm
- **Tab** — Move between modes
- **Escape** — Go a mode back or cancel

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
- **Sales Demo** — Interactive HVAC equipment configurator showcasing hierarchical data navigation with mandatory/optional elements
- **Web Browser** — Accessible web browsing experience
- **Email & Chat** — Integrated communication tools
- **Notebook** — Structured note-taking
- **Mobile Support** — Android and iOS versions

The goal is to provide a comprehensive environment where functionality and accessibility come first — an alternative approach that makes computing more accessible to everyone.

## Development

Plugins generate simple JSON that feeds directly into Sicompass's user interface, making it easy to build extensions in any programming language.

## Community

Join the conversation on [Discord](https://discord.com/channels/1464152138753249313/1464152139231137894).

## License

MIT License — See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Whether it's code, documentation, or feedback, your input helps make computing more accessible for everyone

