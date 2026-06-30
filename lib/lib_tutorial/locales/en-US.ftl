# Tutorial provider strings, English (source/fallback).
# The tutorial is short and guided. See docs/tutorial-guidelines.md.
# Prose rule: no em dashes, no semicolons. Use commas or separate sentences.

tutorial-display-name = tutorial --> here you can go up, down or right

# ---------------------------------------------------------------------------
# 1. Getting Started (guided do-and-confirm)
# ---------------------------------------------------------------------------
tutorial-sec-getting-started = Getting Started --> go right to visit
tutorial-gs-intro = Welcome. This is a short, hands-on start. Do each step as you read it, every step here is safe and you can repeat it as often as you like. Press the Down arrow now to begin.
tutorial-gs-moved = You pressed Down and your screen reader announced this line. Up and Down move the selection through a list. The next item below is a section. Move Down to it, then press the Right arrow to step inside.
tutorial-gs-step = Step inside this section, press Right
tutorial-gs-inside = You stepped in with the Right arrow. This is one level deeper. Everything in Sicompass is a tree of lists you move through this way. Press the Left arrow to go back out.
tutorial-gs-back = Up and Down move between items here, Right goes deeper, and Left comes back. Press Left now to return to Getting Started.
tutorial-gs-checkbox-intro = Welcome back. Now try a checkbox. Move Down to the checkbox below and press Enter. Your screen reader announces checked or unchecked each time.
tutorial-gs-checkbox = <checkbox>Practice checkbox, press Enter to toggle me
tutorial-gs-input-intro = Next, edit some text. Move Down to the line below, press i to start typing, change the text, then press Enter to save or Escape to cancel.
tutorial-gs-input = Edit me, press i or a then Enter: <input>hello world</input>
tutorial-gs-modes = That is the core of getting around. A few more keys to try from here: colon opens commands, Tab searches this list, S starts scroll mode, and w speaks where you are. Press Escape to leave any of them.
tutorial-gs-done = You are ready. The sections below explain how Sicompass works and list every key. Browse them in any order, and press Left at any time to return to the root. Happy navigating.

# ---------------------------------------------------------------------------
# 2. Shortcuts at a glance (the single key reference, grouped by mode)
# ---------------------------------------------------------------------------
tutorial-sec-shortcuts = Shortcuts at a glance
tutorial-sc-intro = Every key, grouped by the mode it works in. Each line leads with the key so it is the first thing spoken. You are in general mode unless you are editing text or inside a prompt.
tutorial-sc-general = General mode
tutorial-sc-gen-updown = Up and Down: move the selection within the current list.
tutorial-sc-gen-rightleft = Right: step into the selected item. Left: step back out to the parent.
tutorial-sc-gen-enter = Enter: activate the selected item, toggle a checkbox, or follow a link.
tutorial-sc-gen-escape = Escape: leave the current mode and return to general mode.
tutorial-sc-gen-page = PgUp and PgDown: move by one line so images and multi-line items are not skipped. Home and End: jump to the first or last item.
tutorial-sc-gen-f5 = F5: refresh the active program, re-fetching the current view.
tutorial-sc-gen-whereami = w: where am I, speak the focus position and the breadcrumb path to it.
tutorial-sc-gen-meta = m: open the meta screen, a navigable list of the keyboard actions and hints available where you are.
tutorial-sc-gen-dashboard = d: show the active program's dashboard image fullscreen, where one is offered.
tutorial-sc-insert = Insert and edit mode
tutorial-sc-in-i = i: insert, place the cursor at the start of the editable text.
tutorial-sc-in-a = a: append, place the cursor at the end of the editable text.
tutorial-sc-in-enter = Enter: confirm and save the edit. Escape: cancel and discard it.
tutorial-sc-in-backspace = Backspace: delete the character before the cursor.
tutorial-sc-command = Command and search
tutorial-sc-cmd-colon = : (colon): open command mode, type a command name and press Enter to run it.
tutorial-sc-cmd-tab = Tab: search and filter the current list as you type.
tutorial-sc-cmd-ctrlf = Ctrl+F: extended search through all children, with results as a flat list you can jump to.
tutorial-sc-cmd-scroll = S: scroll mode, flatten the list and its sublists into one continuous reading view.
tutorial-sc-cmd-history = z: open the read-only history view of this tab's undo timeline.
tutorial-sc-tabs = Tabs and window
tutorial-sc-tab-new = Ctrl+T: open a new tab. Ctrl+W: close the current tab.
tutorial-sc-tab-mru = Ctrl+Tab and Ctrl+Shift+Tab: step through tabs in most-recently-used order.
tutorial-sc-tab-number = Ctrl+1 through Ctrl+9: jump straight to a tab by its number.
tutorial-sc-tab-palette = t: open the tab switcher palette.
tutorial-sc-tab-controls = c: open the window controls palette, minimize, maximize, or close the window.
tutorial-sc-files = Files, undo, and save
tutorial-sc-file-undo = Ctrl+Z: undo. Ctrl+Shift+Z: redo. Each tab keeps its own timeline.
tutorial-sc-file-clipboard = Ctrl+C: copy. Ctrl+V: paste. Delete: delete the selected item.
tutorial-sc-file-save = Ctrl+S: save. Ctrl+Shift+S: save as. Ctrl+O: open a saved configuration file.
tutorial-sc-file-update = Ctrl+U: apply a staged app update.

# ---------------------------------------------------------------------------
# 3. How it works (the mental model, lean)
# ---------------------------------------------------------------------------
tutorial-sec-how-it-works = How it works
tutorial-hiw-tree = Every graphical interface is really a tree of lists. Sicompass makes that structure explicit and navigable entirely from the keyboard, so one set of keys drives everything.
tutorial-hiw-programs = Each item at the root is a program, also called a provider: the file browser, web browser, email, this tutorial. They all plug into the same tree, so once you can navigate one you can navigate them all.
tutorial-hiw-modes = Keys mean different things in different modes. You start in general mode and drop into insert, command, or search mode as needed. The Shortcuts at a glance section lists them all.
tutorial-hiw-editing = Any item that contains an input box is editable. Press i or a to edit it, Enter to save, Escape to cancel. The file browser makes names editable, the settings program makes values editable.
tutorial-hiw-undo = Each tab keeps its own undo timeline. Ctrl+Z walks back through your actions, including the route you navigated, and Ctrl+Shift+Z walks forward. Press z to see the history.
tutorial-hiw-undo-caveats = A few things cannot be undone: a terminal command that already ran, a directory delete larger than 4 MiB once the trash is emptied, and a posted chat message (undo redacts it, so recipients see that it was deleted).
tutorial-hiw-accessibility = Screen reader support is built in through AccessKit on Linux, macOS, and Windows, with nothing to configure. Each item is spoken in its own detected language, so a French line inside an English interface is read with French pronunciation.

# ---------------------------------------------------------------------------
# 4. The programs (one short leaf each)
# ---------------------------------------------------------------------------
tutorial-sec-programs = The programs
tutorial-prog-intro = Sicompass turns each data source into the same navigable tree. Enable or disable programs in Settings under 'Available programs'. Here is what ships with the app.
tutorial-prog-filebrowser = File browser: your filesystem as a tree. Enter directories with Right, rename with i, and create, copy, paste, or delete items inline.
tutorial-prog-texteditor = Text editor: press Right on a file to open its contents as a tree and edit the lines inline. Every change is on the undo timeline.
tutorial-prog-web = Web browser: type a URL in the address bar to load a page as a navigable tree of headings, links, and forms you can fill in.
tutorial-prog-terminal = Terminal: a real shell rendered as a tree, with an input line at the bottom. Full-screen programs like vim and htop get their own interactive mode.
tutorial-prog-chat = Chat: a Matrix client. Log in inline, your rooms appear as a tree, and you type a message and press Enter to send.
tutorial-prog-email = Email: IMAP and SMTP with Gmail OAuth. Folders and messages form a tree, and you can compose, reply, move, and delete, all undoable.
tutorial-prog-salesdemo = Sales demo: an air handling unit configurator that shows how complex, editable, hierarchical data works in Sicompass.
tutorial-prog-remote = Remote services: connect to FFON providers served over HTTP. Set a remote URL and key in Settings, and content is fetched as you navigate.
tutorial-prog-settings = Settings: always the last item at the root. It configures Sicompass and every program, and changes take effect immediately.

# ---------------------------------------------------------------------------
# 5. Interactive playground (hands-on element types)
# ---------------------------------------------------------------------------
tutorial-sec-playground = Interactive playground
tutorial-play-intro = A hands-on playground for the interactive element types Sicompass supports. Try each one as you go.
tutorial-play-checkbox = <checkbox checked>Checkboxes toggle with Enter (this one starts checked)
tutorial-play-button = Buttons run an action with Enter, try this one: <button>demo</button>activate me
tutorial-play-button-pressed = You activated the demo button. This one does nothing, it is here so you can practice.
tutorial-play-input = Inputs edit inline, press i or a then Enter: <input>edit me</input>
tutorial-play-radio-intro = Radio groups let you pick exactly one option. Press Right to enter the group below, then Enter on a choice.
tutorial-play-radio = <radio>Pick a color
tutorial-play-radio-blue = <checked>blue
tutorial-play-radio-green = green
tutorial-play-radio-red = red
tutorial-play-image-intro = Images render inline, and they can carry text before and after. Your screen reader reads the prefix, then the image, then the suffix, so an image is never an unlabelled gap.
tutorial-play-image = Prefix text, then the image: __TEXTURE_JPG__ and suffix text after it.
tutorial-play-link = Press Right to lazy-load this linked file: __FFON_JSON__
tutorial-play-scroll = Scroll mode flattens this list and its sublists into one reading view. Press S, then use PgUp, PgDown, Home, and End on the long passage below.
tutorial-play-lorem = __LOREM_IPSUM__

# ---------------------------------------------------------------------------
# 6. Settings and config
# ---------------------------------------------------------------------------
tutorial-sec-config = Settings and config
tutorial-cfg-file = All configuration lives in one file, ~/.config/sicompass/settings.json, organized by namespace with a section per program.
tutorial-cfg-logs = Sicompass writes a daily log file, sicompass.log, to a platform directory: ~/.local/state/sicompass/ on Linux, ~/Library/Logs/sicompass/ on macOS, and %LOCALAPPDATA%\sicompass\ on Windows. Set the RUST_LOG environment variable to also print logs to the terminal. Logs help when you report a problem.
tutorial-cfg-settings = The Settings program edits everything live: color scheme, display scaling, a shoulder-surfing blank that hides the screen while the screen reader keeps working, and which programs load.
tutorial-cfg-saveload = Some programs support their own config files. Ctrl+S saves, Ctrl+Shift+S saves under a new name, and Ctrl+O opens a saved file.
tutorial-cfg-updates = Sicompass checks for updates at launch. When one is staged, press Ctrl+U to apply it. Plugins update the same way, verified by SHA-256 and an optional ed25519 signature.

# ---------------------------------------------------------------------------
# 7. Extending Sicompass (pointer to the real docs)
# ---------------------------------------------------------------------------
tutorial-sec-extending = Extending Sicompass
tutorial-ext-build = You can build your own programs in Rust, TypeScript, or C. Rust is the standard way, implement the Provider trait from the sicompass-sdk crate and register it, exactly as every built-in program does. TypeScript programs are scripts that output JSON, and C programs are compiled ProviderOps libraries. Install plugins under ~/.config/sicompass/plugins/ and enable them in Settings.
tutorial-ext-docs = The full Provider trait, the ProviderOps interface, the element tags, and a worked example are in the repository docs and the sicompass-sdk crate, not in this tutorial. See the lib/ crates for Rust providers and lib/lib_sales_demo/ for a script provider.
