# Silicon's Compass

*A keyboard-first, accessibility-first way to use your entire computer.*

Sicompass is a free, open-source way to use your computer entirely from the keyboard, with no mouse needed. Under the hood, every screen is just a tree of lists. Sicompass shows you that structure directly, so you move through it with the arrow keys.

You navigate everything the same way, your files, settings, email, and any other data. It is fast, precise, and predictable, it feels the same everywhere, and it works smoothly with screen readers on Linux, macOS, and Windows. It does not try to look pretty, and that is exactly the point.

## Download

Prebuilt packages for Windows, macOS and Linux are on the
[Releases page](../../releases/latest). Pick the one for your system below.

Every download is self-contained. Fonts and shaders are compiled into the
binary, so there is nothing to install alongside it.

### Windows

```powershell
irm https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-installer.ps1 | iex
```

Or download `sicompass-x86_64-pc-windows-msvc.msi` from the Releases page and
double-click it. The installer adds a Start Menu entry and puts sicompass on
your PATH.

### macOS

Download the disk image for your Mac, `aarch64` for Apple Silicon and `x86_64`
for Intel, then drag sicompass to your Applications folder.

```bash
curl -LO https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-aarch64-apple-darwin.dmg
```

The disk image bundles MoltenVK, so there is nothing else to install.

The app is not yet notarized by Apple, so macOS refuses to open it the first
time and reports that it is damaged. Clear the download quarantine flag once
and it starts normally after that.

```bash
xattr -dr com.apple.quarantine /Applications/sicompass.app
```

### Linux

Debian, Ubuntu and Mint:

```bash
curl -LO https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass_0.1.9_amd64.deb
sudo apt install ./sicompass_0.1.9_amd64.deb
```

Fedora, RHEL and openSUSE:

```bash
curl -LO https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-0.1.9-1.x86_64.rpm
sudo dnf install ./sicompass-0.1.9-1.x86_64.rpm
```

Any distribution, no root needed:

```bash
curl -LO https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass_0.1.9_x86_64.AppImage
chmod +x sicompass_0.1.9_x86_64.AppImage
./sicompass_0.1.9_x86_64.AppImage
```

Nix and NixOS:

```bash
nix run github:friendlyflow/sicompass
```

Or install just the binary into `~/.local/bin`, which also works on macOS:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-installer.sh | sh
```

You need a working Vulkan driver, which most desktop distributions already
have. If yours does not, install your vendor's Vulkan package, for example
`mesa-vulkan-drivers` on Debian and Ubuntu. A screen reader also needs
`at-spi2-core` running, which the `.deb` and `.rpm` pull in for you.

### Checking that it worked

```bash
sicompass --check
```

That reports where sicompass found its resources, whether colour emoji can
render, and which graphics devices it can see. It is the first thing to run,
and to paste into a bug report, if the app does not start.

On Windows the app has no console, so send the report to a file instead:

```powershell
$env:SICOMPASS_CHECK_FILE="check.txt"; sicompass --check; type check.txt
```

Every release also ships `SHA256SUMS` files next to the packages, so you can
verify a download before installing it.

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

Nix users can also build the packaged version with `nix build`.

Shaders are compiled by `scripts/gen-shaders.sh` and committed, so building
needs no shader toolchain. Rerun that script if you change anything under
`shaders/`, and `scripts/gen-icons.sh` if you change the icon.

## Key Features

- **Unambiguous Focus**: You always know where the focus is, no guessing
- **Flat Interface**: No popups, dialogs, or overlays, everything is navigated inline within the tree
- **Keyboard-First**: Your hands never leave the keyboard, with tabbed workspaces and letter-driven command palettes
- **Native Accessibility**: Built-in screen reader support on Linux, macOS, and Windows
- **Cross-Platform**: Packaged for Windows, macOS, and Linux, with paths, shells, and PTYs routed through platform helpers
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
