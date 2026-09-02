# Sicompass

*A keyboard-first, accessibility-first way to use your entire computer.*

Sicompass is a free, open-source way to use your computer entirely from the
keyboard, with no mouse needed. Under the hood, every screen is just a tree of
lists. Sicompass shows you that structure directly, so you move through it with
the arrow keys, the same way for your files, settings, email, and any other
data. It is fast, precise, and predictable, it feels the same everywhere, and it
works with screen readers on Linux, macOS, and Windows.

## Download

Every download is self-contained. Fonts and shaders are compiled into the
binary, so there is nothing to install alongside it.

| Platform | Download |
| --- | --- |
| Windows | [sicompass-x86_64-pc-windows-msvc.msi](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-x86_64-pc-windows-msvc.msi) |
| macOS, Apple Silicon | [sicompass-aarch64-apple-darwin.dmg](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-aarch64-apple-darwin.dmg) |
| macOS, Intel | [sicompass-x86_64-apple-darwin.dmg](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-x86_64-apple-darwin.dmg) |
| Debian, Ubuntu, Mint | [sicompass_0.1.18_amd64.deb](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass_0.1.18_amd64.deb) |
| Fedora, RHEL, openSUSE | [sicompass-0.1.18-1.x86_64.rpm](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-0.1.18-1.x86_64.rpm) |
| Any Linux, no root | [sicompass_0.1.18_x86_64.AppImage](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass_0.1.18_x86_64.AppImage) |
| Nix, NixOS | `nix run github:friendlyflow/sicompass` |

Portable archives, if you would rather unpack a binary yourself:
[Windows .zip](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-x86_64-pc-windows-msvc.zip),
[Linux .tar.xz](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-x86_64-unknown-linux-gnu.tar.xz),
[macOS Apple Silicon .tar.xz](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-aarch64-apple-darwin.tar.xz),
[macOS Intel .tar.xz](https://github.com/friendlyflow/sicompass/releases/latest/download/sicompass-x86_64-apple-darwin.tar.xz).
Older versions and the `SHA256SUMS` files are on the
[Releases page](https://github.com/friendlyflow/sicompass/releases).

### Windows

Double-click the `.msi`, then press the Windows key and type `sicompass` to
launch it.

### macOS

sicompass is not yet notarized by Apple, so macOS blocks the download and says
it "could not verify" the file is free of malware. Clear the quarantine flag on
the `.dmg` **before** opening it, in Terminal:

```bash
xattr -dr com.apple.quarantine ~/Downloads/sicompass-aarch64-apple-darwin.dmg
```

Use the `x86_64` file name instead if you are on an Intel Mac. Then open the
`.dmg` and drag sicompass to your Applications folder. It bundles MoltenVK, so
there is nothing else to install, and it starts normally from then on.

Doing it in this order matters. The flag is what macOS copies from the disk
image onto everything you drag out of it, so clearing it first leaves you with
an app that was never quarantined and nothing further to run. If you have
already dragged the app across, clear it there instead:

```bash
xattr -dr com.apple.quarantine /Applications/sicompass.app
```

There is no way to do this from Finder on recent macOS. The right-click and
Open trick no longer works for this warning, and the "Open Anyway" button in
System Settings under Privacy and Security only appears after macOS has
refused the app once.

### Linux

Install the downloaded package with your package manager, so it pulls in what it
needs:

```bash
sudo apt install ./sicompass_0.1.18_amd64.deb      # Debian, Ubuntu, Mint
sudo dnf install ./sicompass-0.1.18-1.x86_64.rpm   # Fedora, RHEL, openSUSE
```

The AppImage needs no root. Make it executable and run it:

```bash
chmod +x sicompass_0.1.18_x86_64.AppImage
./sicompass_0.1.18_x86_64.AppImage
```

You need a working Vulkan driver, which most desktop distributions already have.
If yours does not, install your vendor's package, for example
`mesa-vulkan-drivers` on Debian and Ubuntu.

The `.deb` and `.rpm` pull in everything else, including `at-spi2-core` for
screen readers and `xvfb` for the web browser. The AppImage bundles its own
shared libraries but cannot install `xvfb`, and the plain archive handles
nothing, so with those two you may also need `libpng16`, `zlib`, OpenSSL 3 and
`xvfb`, which is the normal state of any desktop Linux.

### The web browser

Pages are read with a real Chrome or Chromium, which is not bundled, so install
one if you want to use that provider. Chrome never appears on your screen. With
`xvfb` present it runs as a normal browser on an invisible display, which is
what a website expects to see. Without it Chrome runs headless instead, which a
few sites detect and block. If a page refuses to load, install `xvfb` with
`sudo apt install xvfb` or `sudo dnf install xorg-x11-server-Xvfb`.

### The git client

The git client runs the `git` you already have, so install it if you want to use
that provider. Most systems have it. If yours does not, `sudo apt install git` or
`sudo dnf install git` is all it needs. It uses your own git config, your own ssh
agent and your own credential helpers, exactly as your shell would, so a
repository you can push from a terminal is one you can push from here. Nothing is
fetched in the background unless you ask for it in Settings.

### Checking that it worked

```bash
sicompass --check
```

That reports where sicompass found its resources, whether colour emoji can
render, and which graphics devices it can see. It is the first thing to run, and
to paste into a bug report, if the app does not start. On Windows the app has no
console, so send the report to a file instead:

```powershell
$env:SICOMPASS_CHECK_FILE="check.txt"; sicompass --check; type check.txt
```

## Building from source

```bash
nix develop          # optional, brings the whole toolchain
cargo build --release
cargo run --release
```

Nix users can also build the packaged version with `nix build`. Shaders are
compiled by `scripts/gen-shaders.sh` and committed, so building needs no shader
toolchain. Rerun that script if you change anything under `shaders/`, and
`scripts/gen-icons.sh` if you change the icon.

## Key features

- **Unambiguous focus**: you always know where the focus is, no guessing
- **Flat interface**: no popups, dialogs, or overlays, everything is navigated inline within the tree
- **Keyboard-first**: your hands never leave the keyboard, with tabbed workspaces and letter-driven command palettes
- **Native accessibility**: built-in screen reader support on Linux, macOS, and Windows
- **Cross-platform**: packaged for Windows, macOS, and Linux, with paths, shells, and PTYs routed through platform helpers
- **High-performance rendering**: Vulkan graphics with a FreeType2 glyph atlas
- **Extensible**: provider-based plugin system with a built-in store for hot enable/disable

## Built-in providers

Each provider turns a different data source into the same keyboard-navigable
tree: File Browser, Text Editor, Notes, Email, Chat, Web Browser, Terminal, Git
Client, Plugin Store, and Settings.

## Community

Join the conversation on
[Discord](https://discord.com/channels/1464152138753249313/1464152139231137894).

## License

<!-- #### Commercial license

If you want to use Sicompass to develop commercial projects and applications, the Commercial license is the appropriate license. With this option, your source code is kept proprietary.
[Read more about the commercial license](https://sicompass.org/license/) -->

#### Open source license

If you are creating an open source application under a license compatible with
the GNU GPL license v3, you may use this project under the terms of the GPLv3.

## Contributing

Contributions are welcome. Whether it is code, documentation, or feedback, your
input helps make computing more accessible for everyone.
