# Silicon's Compass

From around 1981 until 2026, the way computers look and work on screen (the graphical user interface, or GUI) barely changed, and it never worked well for people who need accessibility. That changes now.

Sicompass is a free, open-source way to use your computer entirely from the keyboard, with no mouse needed. Under the hood, every screen you see is really just a tree of lists. Sicompass shows you that structure directly, so you move through it with the arrow keys.

You navigate everything the same way: your files, your settings, your email, and any other data. For people building software, that means less work and fewer bugs. For people using it, it means a fast, precise, and predictable experience that feels the same everywhere. It also works smoothly with screen readers on Linux, macOS, and Windows.

It does not try to look pretty, and it works very differently from web pages and HTML, and those are exactly the reasons it exists.

## Key Features

- **Flat Interface**: No popups, dialogs, or overlays, everything is navigated inline within the tree
- **Keyboard-First Design**: Faster and handier navigation because your hands never leave the keyboard
- **Native Accessibility**: Built-in support for screen readers on Linux, macOS, and Windows
- **Cross-Platform**: Shipped and tested on Ubuntu today. Paths, shells, and PTY plumbing route through platform helpers (XDG / `~/Library` / `%APPDATA%`, `bash`/`zsh`/`fish`/`pwsh`/`cmd.exe`, `forkpty`/ConPTY), with packaged macOS and Windows releases planned
- **High-Performance Rendering**: Vulkan-powered graphics with FreeType2/HarfBuzz text shaping
- **Extensible Architecture**: Provider-based plugin system with a built-in plugin store for hot enable/disable
- **Lower Development Cost**: Because the user-interface elements are decided beforehand, design never has to be programmed. Development is navigation-first instead of design-first, which cuts complexity and speeds up delivery

## Built-in Providers

Sicompass comes with several providers out of the box, each turning a different data source into the same keyboard-navigable tree:

- **File Browser**: Navigate your filesystem as a navigable tree with inline rename, copy, paste, and delete (delete-undo restores from a content snapshot when the OS trash is empty)
- **Text Editor**: Browse the filesystem and open files as language-aware FFON trees, with inline line editing and create/delete/rename, every change, lines included, recorded on the unified undo timeline
- **Email**: IMAP/SMTP client with Google OAuth, Cc/Bcc, attachments, drafts, flag/move/delete (typed `ImapOp` undo by Message-ID so the operation survives folder moves), threaded history, and FFON-bodied messages
- **Chat**: Matrix client with public and private rooms, invites, member management (leave/kick/ban are undoable), encrypted messages, unread badges, and a background sync thread
- **Web Browser**: Browse the web with HTML-to-FFON conversion. Fill and submit forms via the Chrome DevTools Protocol. Cookie-consent banners are auto-accepted
- **Terminal**: Interactive shell backed by a vte PTY with a synthesized prompt and an input slot that doubles as a browsable command-history list (arrow into it to recall and re-run past commands). Auto-switches to a fullscreen interactive dashboard the moment a child program enters the alt-screen (vim, htop, less, …) and routes every key straight to the TUI. Cross-platform shell selection (`$SHELL` on Unix, `%ComSpec%` on Windows) with platform-appropriate prompts
- **Sales Demo**: Interactive HVAC equipment configurator showcasing hierarchical data navigation with inline editing and diagram view
- **Plugin Store**: Enable and disable providers on the fly with checkbox toggles, no restart needed
- **Settings**: Configure color scheme, display scaling, shoulder-surfing protection, loaded programs, and provider-specific options in a unified settings tree

## The Vision

Sicompass is the foundation for a unified, accessible platform. Future development includes:

- **Notes**: Structured note-taking with server-side sync
- **IDE**: Code as a navigable structure, with Rust code generation
- **Mobile Support**: Android and iOS versions
- **Login Manager and Desktop Environment**: A fully keyboard-driven Linux session
- **Screen Reader and Braille Display**: D-Bus in- and output on Linux that drives the users of these technologies

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

A hierarchy is usually represented top to bottom, but you can also represent it left to right, and that representation of the hierarchy yields a form that is very navigation-friendly: CEO → right → CSO, COO, CTO, CHRO, ... → right → middle management of each C-level → left → back to parent, i.e. CSO, COO, ..., and then up and down within each list. So the four arrow keys, escape and enter. Escape returns from any other mode back to general mode and enter confirms input content, checks the checkbox, checks the radio button, and presses a button.
There are also various other modes: tab (a list search mode) being one of them, another is the extended search Ctrl+F, which also searches in children. There is also scroll mode (the `S` key), which flattens a list together with all its sublists into one long, scrollable reading view where each element is shown with a `layer: X list: Y/Z` header above its content.
When assemblin Sicompass, vim and emacs were also looked at, keyboard-only technologies that nonetheless cannot control the entire computer, and where various shortcuts have a function: for example, a for append, which places the caret at the last position in an input field, i places the caret at the first position, and are more modes available.

Another major advantage is the development cycle, which can be a lot shorter as the graphical user interface's elements are already foreseen. Only a ffon (subset of json) needs to be served to Sicompass in order to assemble the GUI and Accessible structure. That ffon consists only of content and an indication of what element to setup.

## Community

Join the conversation on [Discord](https://discord.com/channels/1464152138753249313/1464152139231137894).

## License

#### Commercial license

If you want to use Sicompass to develop commercial projects and applications, the Commercial license is the appropriate license. With this option, your source code is kept proprietary.
[Read more about the commercial license](https://sicompass.org/license/)

#### Open source license

If you are creating an open source application under a license compatible with
the GNU GPL license v3, you may use this project under the terms of the GPLv3.

## Contributing

Contributions are welcome! Whether it's code, documentation, or feedback, your input helps make computing more accessible for everyone

