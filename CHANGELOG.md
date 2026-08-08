# Changelog

cargo-dist parses this file and uses the matching version's section as the
GitHub Release body, so entries here are what users read on the download page.

## 0.1.12

### Every program's files now travel inside the binary

The tutorial's image and its example document, and the sales demo's diagram,
used to be loose files shipped alongside the executable. Whether they arrived
depended on four separate packaging lists agreeing with each other, and nothing
checked that they did. When they disagreed nothing failed at build time, it just
meant a tutorial with a missing picture, and that is exactly what happened for
several releases in a row.

They are now compiled into the executable, like the fonts and the shaders
already were. There is nothing left to install beside the binary and nothing
left to go missing, on any platform.

`sicompass --check` reports this directly: it lists each program's files with
the number of bytes it actually found, rather than reporting which directory it
guessed at.

### Plugins can ship their own files

A plugin can now put files in an `assets` folder next to itself and either read
them itself or hand them to sicompass to display. It reaches nothing else: a
plugin cannot read a file outside its own folder, cannot follow a shortcut that
leads out of it, and cannot see whether some other file on your computer exists.

This also closes a hole. A plugin used to be able to name any picture on your
computer and have sicompass open it. It can only name its own now.

Plugin authors need `sicompass-pdk` 0.2.0 for the new call. Existing plugins keep
working unchanged.

## 0.1.11

### Linux: the arrow keys keep working after a web page loads

With a screen reader running, opening a page in the web browser left the arrow
keys dead. Navigation only came back after switching away from sicompass and
back again.

sicompass loads pages in a Chrome that it starts on an invisible display, so
that no browser window ever appears on screen and takes the focus. That hides
Chrome from the screen, but not from the screen reader, because the
accessibility bus belongs to the whole login session rather than to one
display. Orca was therefore offered a second application called "Google
Chrome", carrying a window named after the page that had just been opened, and
it had somewhere else to go. Chrome only announces itself this way while an
assistive technology is running, which is why the browser behaved perfectly
until a screen reader was switched on.

That Chrome is now kept off the accessibility bus entirely, so the screen
reader sees only sicompass and the arrow keys keep going where they are aimed.

### macOS: the app starts on a Mac that does not have Homebrew

0.1.10 could not launch on most Macs. It quit immediately, and Console showed
a crash report ending in "Library not loaded:
/opt/homebrew/opt/freetype/lib/libfreetype.6.dylib".

The build machine has Homebrew and its FreeType package installed, and the
released binary ended up pointing at that copy by its full path on the build
machine. On any Mac without that exact package the path does not exist, so
macOS stopped the app before it ran a single line. Installing FreeType by hand
was the only workaround.

FreeType is now built into the binary on macOS, as it already was on Linux and
Windows, so there is nothing left to install. The release now also refuses to
publish a macOS build that points at anything outside the app bundle and macOS
itself, which is the check that was missing.

If you worked around this with `brew install freetype`, you no longer need it
for sicompass.

## 0.1.10

### Ctrl+O: arrow keys work again, and Enter opens the file you selected

In the open dialog the arrow keys appeared dead for the first few presses
before suddenly jumping, and pressing Enter on the first file reported
"Please select a .json file".

The dialog lists only `.json` files and folders, but the cursor still counted
every entry in the folder, including the ones it was hiding. So the cursor and
the highlight drifted apart: arrows moved the cursor onto hidden files while
the highlight stayed put, and Enter acted on whatever hidden file the cursor
had landed on rather than on the highlighted one.

The highlighted row is now what the cursor and Enter follow. Arrow keys, Home,
End, Page Up, Page Down, Right into a folder and search inside the dialog all
move one visible entry at a time.

This was most visible on macOS, where `.DS_Store` sorts to the top of nearly
every folder and so was always the entry the cursor started on, but any
non-JSON file ahead of a JSON one triggered it on every platform.

A folder with no `.json` files now says so instead of reporting the file the
cursor happened to be parked on.

### Symlinked folders can be opened again

A folder reached through a symlink was listed as a file, so the right arrow
would not enter it and Ctrl+O could not save into it or open from it. On macOS
that covered `/tmp` and `/var`, and on Linux distributions that merge `/usr` it
covered `/bin`, `/lib` and `/sbin`.

Symlinks are now followed when deciding whether an entry is a folder, which is
what deleting and copying already did. The properties view still describes the
link itself, so it keeps its leading `l`.

### The file browser hides dotfiles

Entries starting with a dot are no longer listed by default, so folders are no
longer led by `.DS_Store` and friends. Extended search skips them too, so it no
longer walks the whole of `.git`.

Run `show/hide hidden files` from the `:` command list to show them again, and
run it a second time to hide them.

### A page that fails to load no longer sticks on "Loading…"

When the browser could not start, the error was recorded but the page content
never was, so the view had nothing to redraw and sat on "Loading…" forever with
the reason never shown. Both failure paths now publish the error along with the
content, so the page reports what went wrong.

If Chrome is found on a mounted disk image rather than installed, the "not
found" message now names that copy, which is the usual reason a freshly
downloaded Chrome is not picked up.

### Navigating while a page is still loading no longer drops the load

Committing a URL while another page was loading updated the address bar and
then discarded the request, so the browser kept showing the old page. Requests
are now queued while a load is running, the newest one wins, and the running
load picks up the queue when it finishes. No navigation is silently lost.

### macOS no longer shows Chrome windows on screen

The web browser provider launched Chrome headed on macOS, the only platform
that did, so its windows appeared on the desktop and in the window switcher.
It now runs headless there like everywhere else. Linux uses Xvfb and falls back
to headless, and Windows parks the window off-screen.

### VoiceOver now works on macOS

sicompass was silent under VoiceOver. Orca and NVDA were unaffected.

The accessibility tree was built once for all three platforms, using the two
mechanisms Orca and NVDA listen to: mutating one node's label, and a label-only
live region. NSAccessibility honours neither. It announces on a live region's
*value*, which was never set, and it re-reads the focused element only when the
focused node id changes, which it never did.

macOS now gets its own tree. Everything spoken goes through a single live
region, so the list, mode and error announcements, the typed-character echo,
and the `w` position report are all read out.

Set `SICOMPASS_A11Y_DEBUG=1` to trace the adapter if speech ever goes missing.

### The `curl | sh` and `irm | iex` installers are gone

`sicompass-installer.sh` and `sicompass-installer.ps1` are no longer built. Both
came from cargo-dist templates aimed at command-line tools: they copied a binary
and edited PATH, with no way to install anything the app depends on. That is
fine for a CLI and not enough for a windowed app, which needs a Vulkan loader,
`at-spi2-core` for screen readers, and `xvfb` for the web browser on Linux, and
MoltenVK on macOS.

Use the package for your system instead. Every platform has one that declares
its own dependencies:

- **Windows**: the `.msi`, which also adds the Start Menu entry the PowerShell
  installer never could.
- **macOS**: the `.dmg`, which bundles MoltenVK.
- **Linux**: the `.deb`, `.rpm`, AppImage, or `nix run
  github:friendlyflow/sicompass`.

The portable `.tar.xz` and `.zip` archives are unchanged, for anyone who would
rather unpack a binary by hand.

## 0.1.9

### Downloads for Windows, macOS and Linux

Every platform now has a real installer. Previously only Windows had a package,
and it could not actually start.

- **Windows**: a PowerShell one-liner, or an `.msi` that adds a Start Menu
  entry and an application icon.
- **macOS**: a `.dmg` for Apple Silicon and one for Intel, each bundling
  MoltenVK so there is nothing else to install. Also reachable with
  `curl ... | sh`.
- **Linux**: `.deb`, `.rpm`, AppImage, a `curl ... | sh` installer, and
  `nix run github:friendlyflow/sicompass`.

### The published binary now starts

Releases up to and including 0.1.8 shipped an executable that read its shaders
and fonts from the working directory and shipped neither, so it exited at
startup unless launched from a source checkout.

- Shaders are compiled to SPIR-V and embedded in the binary.
- Fonts are embedded in the binary.
- The `assets/` tree is now found relative to the executable, so it works from
  an archive, an `.app` bundle, a `.deb`, an AppImage, the `.msi` and Nix
  without any wrapper script.

### macOS support

macOS was previously unreleasable. `ash::Entry::load()` looks for
`libvulkan.dylib`, which no Mac has, and the Vulkan instance was created
without the portability flags MoltenVK needs, so it would have found no GPU
even after loading.

- Vulkan is now loaded from the bundled MoltenVK, the LunarG SDK, or Homebrew,
  in that order.
- `VK_KHR_portability_enumeration` and `VK_KHR_portability_subset` are enabled
  where required.
- The app is ad-hoc signed. It is not yet notarized, so clear the quarantine
  flag once after downloading: `xattr -dr com.apple.quarantine
  /Applications/sicompass.app`

### Colour emoji now render

Emoji in chat and email content were silently invisible on every shipped
build. FreeType only decodes Noto Color Emoji's PNG bitmap strikes when it is
compiled with PNG support, and the release builds were falling back to a
vendored FreeType that is not, so glyphs came back zero-sized rather than as
an error. All three platforms now build against a PNG-capable FreeType, and
`sicompass --check` reports whether yours does.

### Changed

- The default monospace face is now DejaVu Sans Mono. The previous face was
  Microsoft Consolas, which cannot be redistributed, and which in practice
  covered only 600 codepoints with no box-drawing glyphs, so most of the
  interface was already being drawn from the DejaVu fallback.
- The application has an icon, on every platform.
- Windows release builds no longer open a console window behind the app.
- `sicompass --check` now reports where resources resolved to, whether colour
  emoji can render, and which Vulkan devices are visible, instead of checking
  for files that no longer exist on disk. It is the first thing to run when
  the app will not start. Set `SICOMPASS_CHECK_FILE` to also write the report
  to a file, which is the only way to read it on Windows, where the app has no
  console.
- `No suitable Vulkan GPU found` now says how many devices were enumerated,
  which distinguishes a driver problem from a capability problem.

### Fixed

- `bundled-sdl3` builds on Linux. The feature enabled `sdl3-sys/no-sdl-libc`
  on every platform, which un-defined `HAVE_STDIO_H` in SDL's build and broke
  the `<errno.h>` include in `SDL_iostream.c` under GCC. It is now applied only
  on Windows, where it is genuinely needed. CI builds the same way the release
  does as a result.
