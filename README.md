# Silicon's Compass

From around 1981 until 2026 the graphical user interface (GUI) marginally evolved while never being able to service accessibility the way many people demanded.

Proudly I can share that that changes now, that non-inclusive era is history ...

(Sorry for the marginal 'design' approach and not-sorry for the optimal navigation approach, which differs maximally from html and its eco-system, but those facts are its main reasons of existence.)

## What is Sicompass?

Sicompass is an open-source, keyboard-only interface for navigating and editing structured content. Every graphical interface is essentially a tree of lists — Sicompass makes that structure explicit and navigable entirely from the keyboard, eliminating the need for a mouse.

This consistent unified interaction model means one consistent way to navigate everything: files, settings, and any data source built on top of it. For developers, it means simpler UI development with fewer bugs. For users, it means a unified, fast, precise and handy experience that works the same everywhere.

Built for users who prefer or require keyboard-driven interaction, Sicompass offers intuitive keyboard navigation and seamless screen reader integration through AccessKit.

## Key Features

- **Flat Interface** — No popups, dialogs, or overlays — everything is navigated inline within the tree
- **Keyboard-First Design** — Faster and handier navigation because your hands never leave the keyboard
- **Native Accessibility** — Built-in support for screen readers on Linux, macOS, and Windows
- **Cross-Platform** — Currently developed and tested on Ubuntu, with macOS and Windows releases planned for later
- **High-Performance Rendering** — Vulkan-powered graphics with FreeType2/HarfBuzz text shaping
- **Extensible Architecture** — Provider-based plugin system with a built-in plugin store for hot enable/disable
- **Simpler UI Development** — Functionality over design means less complexity and faster development with minimal styling, design doen't need to be programmed as it's already worked out

## Built-in Providers

Sicompass comes with several providers out of the box, each turning a different data source into the same keyboard-navigable tree:

- **File Browser** — Navigate your filesystem as a navigable tree with inline rename, copy, paste, and delete
- **Email** — Integrated communication tools using IMAP/SMTP protocol
- **Web Browser** — Browse the web with HTML-to-FFON conversion, turning web pages into keyboard-navigable trees
- **Sales Demo** — Interactive HVAC equipment configurator showcasing hierarchical data navigation with inline editing and diagram view
- **Plugin Store** — Enable and disable providers on the fly with checkbox toggles, no restart needed
- **Settings** — Configure color scheme, loaded programs, and provider-specific options in a unified settings tree

## The Vision

Sicompass is the foundation for a unified, accessible platform. Future development includes:

- **Chat** — Integrated communication tools using Matrix protocol
- **Notes** — Structured note-taking with server-side sync
- **IDE** — Code as a navigable structure, with Rust code generation
- **Terminal** — A terminal emulator integrated as a provider
- **Mobile Support** — Android and iOS versions
- **Login Manager and Desktop Environment** — A fully keyboard-driven Linux session
- **Screen Reader and Braille Display** — D-Bus in- and output on Linux that drives the users of these technologies

The goal is to provide a comprehensive environment where functionality and accessibility come first (not design), an alternative approach that makes computing more accessible to everyone.

### Building from Source

```bash
# Optional: use Nix for dependency management
nix develop

cargo build --release
```

### Intermezzo

There are two ways to set up a good GUI. The first is the one where design takes priority, and that is the GUI that has been dominant for over 45 years. The other one, which has never been fully developed, is the GUI where structure and keyboard navigation take priority. The first has never been sufficiently usable for blind users, like eg. an HTML page with non-semantic divs and spans.

In that first type of GUI, two different structures/hierarchies are used to make it accessible: the first to organize the graphical elements, and from that the second is derived to drive the screen reader/braille display. That first structure is sometimes built up dynamically, which in my opinion can lead to a flawed synchronization between both structures. So I merged those two structures into a single structure/hierarchy in Sicompass, but then navigation also had to follow suit, and since the mouse is unusable for blind people, navigation had to be exclusively via keyboard.

A hierarchy is usually represented top to bottom, but you can also represent it left to right, and that representation of the hierarchy yields a form that is very navigation-friendly: CEO → right → CSO, COO, CTO, CHRO, ... → right → middle management of each C-level → left → back to parent, i.e. CSO, COO, ..., and then up and down within each list. So the four arrow keys, escape and enter. Escape returns from any other mode back to operator mode and enter confirms input content, checks the checkbox, checks the radio button, and presses a button.
There are also various other modes: tab (a list search mode) being one of them, another is the extended search Ctrl+F, which also searches in children.
When assemblin Sicompass, vim and emacs were also looked at, keyboard-only technologies that nonetheless cannot control the entire computer, and where various shortcuts have a function: for example, a for append, which places the caret at the last position in an input field, i places the caret at the first position, and are more modes available.

Another major advantage is the development cycle, which can be a lot shorter as the graphical user interface's elements are already foreseen. Only a ffon (subset of json) needs to be served to Sicompass in order to assemble the GUI and Accessible structure. That ffon consists only of content and an indication of what element to setup.

## Community

Join the conversation on [Discord](https://discord.com/channels/1464152138753249313/1464152139231137894).

## License

MIT License — See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Whether it's code, documentation, or feedback, your input helps make computing more accessible for everyone

