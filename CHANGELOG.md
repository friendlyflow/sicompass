# Changelog

cargo-dist parses this file and uses the matching version's section as the
GitHub Release body, so entries here are what users read on the download page.

## 0.1.18

### Claude starts in the folder you are standing in

The Claude program used to open straight into a conversation, and that
conversation ran wherever Sicompass itself had been started from. Which folder
that was depended on how you launched the app, and from a desktop icon it was
usually somewhere useless. It mattered more than it sounds, because Claude works
out what it knows about your project from the folder it runs in: the project's
CLAUDE.md, its skills, its settings and hooks all come from there. Started in the
wrong place, it had none of them, and there was no way to tell it otherwise.

Claude now opens on a list of folders instead, rooted at the top of the
filesystem, the same as the terminal. Walk to a project with the arrow keys, and
press `:` to start a session in the folder you are standing in, whose contents
the list is showing. Wanting Claude a level deeper is a Right away. Escape
returns to the folders with the conversation still running, so you can look
something up and come straight back to it.

Pressing `:` in a different folder starts a fresh session there. Claude fixes its
working directory when it starts and cannot be moved afterwards the way a shell
can be walked around with `cd`, so continuing the old conversation in a new
project would mean answering questions about a tree it cannot see. The prompts
you have typed are kept for recall either way.

Two smaller consequences. Opening the program no longer starts Claude at all, so
enabling it costs nothing until you press `:`. And closing a tab while Claude is
still working now asks first, the way it already did for a terminal running a
command, since closing takes the conversation with it.

### Multilingual sites list their languages instead of asking first

0.1.17 could interrupt a page with a list of languages to choose from before it
would show anything. It was meant for a site that genuinely will not continue
until you have chosen, but the test for that was only "this page mentions more
than one language", which is true of almost every Belgian site. So elevenways.be
and anysurfer.be, among many others, replaced their own content with a question
nobody had asked.

The lists it offered were wrong as often as not. A site usually marks the
language you are already reading differently from the ones you could switch to,
so anysurfer.be, in Dutch, offered only English and French, with no way to stay
where you were. On elevenways.be the language markers scattered over ordinary
content links leaked into the list, so the Dutch entry led to the contact page.

The question is gone. The address you asked for is the address that loads. The
`choose language` command went with it, and if you had already answered the
question once, `clear cookies` now clears that too. Cookie banners are
unchanged: those really do sit between you and the page, so they are still
answered as a step of their own, and nothing is accepted on your behalf.

A site's own language links are read as part of the page, which is where you
would look for them. That turned out not to be enough on its own. On bpost.be
the only language switcher is a pop-up the site keeps hidden until it decides to
show it, marked in a way that tells a screen reader to ignore it, and built from
links that do not actually go anywhere. So it was read as nothing at all, and
bpost, which offers four languages, ended up offering none.

Every page now ends with a `languages` section listing the versions the site
declares it has, each one a link you can follow. It is the last thing on the
page rather than the first, so you meet the page before its language list, and
it is always in the same place instead of wherever the site chose to put its
switcher. The language you are reading is listed too, marked as the current one,
which is what anysurfer.be leaves out. A site that says nothing about other
languages grows nothing.

### The address bar remembers where you have been

Every address you load is kept, newest first, as rows under the web browser's
address bar. Press Enter on one to go back to that page, and it returns to the
top of the list, so the handful of sites you actually use stay within a keypress
or two. The list is written to disk, so it survives a restart, and tabs that
were open at the same time merge their additions rather than overwriting each
other.

Not everything worth keeping is somewhere you go often, so `b` marks the row
under the cursor as a bookmark. It also works on the page you are reading, which
matters most for a page you arrived at by following a link, since that address
was never typed and would otherwise be hard to name. Bookmarked rows are
announced with `[bookmark]` in front of them, and `b` again removes the mark.

The history has a size limit, `URL history` under web browser in Settings,
default 50000 addresses. Bookmarks are exempt from it. Trimming the list to fit
drops old addresses, never a bookmarked one, so a bookmark is the way to say
that an address should outlive your ordinary browsing.

### The colon commands now say which one you are in

Pressing `:` opens a command mode, and until now only the file browser said so.
In the terminal and in Claude, `:` turns the folder list into a live shell or
session, and the header went on reading "general mode" the whole time you were in
there. Claude's second `:`, the one that lists the project's skills, called itself
"insert mode", which is the name of a completely different mode and sounded
identical to it in every language.

Each colon layer now has its own name, in the header and in the `w` "where am I"
announcement, which always agree. The file browser's palette is `command mode`, as
before. The terminal's shell is `command mode` too, because that is the only layer
it has. Claude's session is `first command mode`, which is how you know a second
`:` is waiting, and the skills list it opens is `second command mode`. Entering and
leaving a shell or a session now speaks the new mode name as well, instead of only
reading out the row you landed on.

One thing this uncovered: in the file browser, `:` had been opening Claude's
insert palette rather than the command palette whenever the last row of a folder
was a subfolder. The commands still ran, so it went unnoticed, but the palette
announced the wrong name and showed the wrong prompt. It opens the command palette
now, always.

### The tutorial caught up with the app

The in-app tutorial had drifted since the browser, the file browser and the
plugin system all changed under it. It now covers the browser's recall history,
bookmarks and its `clear cookies` and `show hidden content` commands, and the
keys that had never been listed at all: cut, copy-value,
forward delete, structural insert, and text selection. Two sections were also
simply wrong. Plugins are WASM components, not TypeScript scripts or compiled C
libraries, and the terminal's `:` opens a shell in the folder you are reading
rather than the one under the cursor.

## 0.1.17

### Web pages read in the order you see them, grouped into regions

The browser used to read a page in the order the site's templating engine wrote
the markup, which is often nothing like the order the page appears on screen.
A footer written first and shown last was read first, and a page with no
landmarks arrived as one long flat run of everything on it.

Chrome has already worked out where every element sits, so the browser now asks
it, and uses the answer twice. Regions that read as navigation, main content,
complementary and footer are grouped under those names, so you can move between
them instead of scrolling past them. Within a region, anything laid out in a
different order from the markup is now read in the order it is laid out.

On bpost.be the front page went from nineteen top level entries, with the
article stranded eighth among them, to three: the skip link, the content, and
the navigation.

Nothing here guesses when it is unsure. If a site already marks up its own
landmarks, those are used untouched. If the page is too small, too large, or
too short of layout information to be confident about, it is read exactly as
before. `show hidden content` still turns the whole pass off and reloads, so
the original order is always one command away.

### A hidden menu could send what you typed to the wrong form

A menu the site keeps hidden until you open it, a login dropdown for example, is
kept and moved after the content so its heading does not swallow the page. When
such a menu contained a form, moving it renumbered every form after it, so text
typed into the first visible field could be filled into the hidden form instead.
Search boxes on pages with a hidden login panel were the common case. The
numbering now stays fixed wherever the menu is moved to.

### macOS: 0.1.14 could not draw anything, and this fixes it

0.1.14 started and then failed to find any graphics device, so nothing rendered.
Signing the app for the first time also switched on a stricter macOS mode that
only lets a program load libraries carrying the same signature as itself. The
graphics library bundled inside the app is signed separately, so the app was
refused its own copy of it, and on macOS that library is the entire graphics
stack.

The app now carries the permission that says it is allowed to load a library it
ships itself, which is the normal arrangement for an app that bundles one.

**If you are on 0.1.14, update.** Nothing else in it worked around this.

The release now refuses to publish a macOS build whose graphics library does not
load, rather than only checking a narrower failure as it did before. That check
is what should have caught this.

## 0.1.14

### The macOS download installs and opens

The `.dmg` shipped a copy of the app that was never signed. The signing step ran
after the disk image had already been sealed around the unsealed copy, so the
loose bundle on the build machine was correct and the one people downloaded was
not. It was checked in the same place it was correct, which is why it survived
several releases.

Clearing the quarantine flag failed too. The bundled MoltenVK library arrived
read-only, and `xattr` cannot clear the flag on a file it is not allowed to
write, so the one command in the README stopped with "Permission denied" on it.
That was the one step nobody could skip, because the app does not open until the
flag is gone.

Both are now checked by opening the finished `.dmg` and inspecting the app
inside it, rather than the copy left behind on the build machine.

### `--check` no longer reports a working Mac as broken

`sicompass --check` asked the graphics driver for a feature that only the Vulkan
loader provides. The installed app talks to MoltenVK directly and has no loader,
so the request was refused and the report claimed no graphics device could be
found, on installations that were rendering perfectly well. Rendering itself was
never affected.

### Every installer is listed on the download page

The `.dmg`, `.deb`, `.rpm` and AppImage were attached to each release but named
nowhere in its text, which listed only the portable archives. Since the app's
update prompt opens that page, anyone following a notification on macOS was
offered a `.tar.xz` while the `.dmg` sat out of sight further down. The page now
leads with the right download for each platform, and repeats the macOS
quarantine command next to it.

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
