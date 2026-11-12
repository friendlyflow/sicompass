# Tutorial provider strings, English (source/fallback).
# The tutorial is short and guided. See docs/tutorial-guidelines.md.
# Prose rule: no em dashes, no semicolons. Use commas or separate sentences.

tutorial-display-name = tutorial

# ---------------------------------------------------------------------------
# 1. Getting started (guided do-and-confirm)
# ---------------------------------------------------------------------------
tutorial-sec-getting-started = Getting started
tutorial-gs-intro = Welcome. This is a short, hands-on start. Do each step as you read it, every step here is safe and you can repeat it as often as you like. Press the Down arrow now to begin.
tutorial-gs-moved = You pressed Down and your screen reader announced this line. Up and Down move the selection through a list. The next item below is a section. Move Down to it, then press the Right arrow to step inside.
tutorial-gs-step = Step inside this section, press Right
tutorial-gs-inside = You stepped in with the Right arrow. This is one level deeper. Every screen in Sicompass is one list, a layer of a larger hierarchy, and you move between the layers this way. Press the Left arrow to go back out.
tutorial-gs-back = Up and Down move between items here, Right goes deeper, and Left comes back. Press Left now to return to Getting started.
tutorial-gs-checkbox-intro = Welcome back. Now try a checkbox. Move Down to the checkbox below and press Enter. Your screen reader announces checked or unchecked each time.
tutorial-gs-checkbox = <checkbox>Practice checkbox, press Enter to toggle me
tutorial-gs-input-intro = Next, edit some text. Move Down to the line below, press i to start typing, change the text, then press Enter to save or Escape to cancel.
tutorial-gs-input = Edit me, press i or a then Enter: <input>hello world</input>
tutorial-gs-modes = That is the core of getting around. A few more keys to try from here: colon opens commands, F searches this list, Tab opens the beginner's manual, S starts scroll mode, and Ctrl+W speaks where you are. Press Escape to leave any of them.
tutorial-gs-done = You are ready. The sections below explain how Sicompass works and list every key. Browse them in any order, and press Left at any time to return to the root. Happy navigating.

# ---------------------------------------------------------------------------
# 2. Shortcuts at a glance (the single key reference, grouped by mode)
# ---------------------------------------------------------------------------
tutorial-sec-shortcuts = Shortcuts at a glance
tutorial-sc-intro = Every key, grouped by the mode it works in. Each line leads with the key so it is the first thing spoken. You are in general mode unless you are editing text or inside a prompt.
tutorial-sc-general = General mode
tutorial-sc-gen-updown = Up and Down: move the selection within the current list.
tutorial-sc-gen-rightleft = Right: step into a row with a right arrow. Left: step back out to the parent, the name with the left arrow.
tutorial-sc-gen-enter = Enter: activate the selected item, toggle a checkbox or pick a radio choice, follow a link, show a picture big, or play a video. In the big view, Enter pauses and resumes a video.
tutorial-sc-gen-in-element = Right on a row with several links, pictures, buttons or videos: in element mode, at the first of them. Left and Right move between them, Up and Down move a line, PgUp and PgDown move a screen, Home and End go to the start or end of the line they are on (a row of a grid), Ctrl+Home and Ctrl+End to the first or last, Escape leaves. Right on the last one goes into the row, if it has a right arrow. Such a row starts with an arrow right, and Enter outside the mode does what Enter in it does on the first. On a checkbox or a radio choice, Enter changes the row, so Right is how you reach even one link or button in it.
tutorial-sc-gen-escape = Escape: leave the current mode and return to general mode.
tutorial-sc-gen-page = PgUp and PgDown: move a screen, to the line at the same height on it, so a picture or a long item is never skipped. On the last or first screen they go to the last or first item. Home and End, or Ctrl+Home and Ctrl+End: jump to the first or last item. Each says where you landed.
tutorial-sc-gen-f5 = F5: refresh the active program, re-fetching the current view.
tutorial-sc-gen-whereami = Ctrl+W: where am I, speak the focus position and the breadcrumb path to it. It works from any mode, even while typing.
tutorial-sc-gen-meta = m: open the meta screen, a navigable list of the keyboard actions and hints available where you are. Type to narrow it down.
tutorial-sc-gen-dashboard = d: show the active program's dashboard image fullscreen, where one is offered.
tutorial-sc-gen-bookmark = b: bookmark the item you are on, where the program offers bookmarks. The web browser marks the page you are reading, or the address under the cursor.
tutorial-sc-gen-insert = Ctrl+I: insert a new item before the selected one. Ctrl+A: insert one after it.
tutorial-sc-insert = Insert and edit mode
tutorial-sc-in-i = i: insert, place the cursor at the start of the editable text.
tutorial-sc-in-a = a: append, place the cursor at the end of the editable text.
tutorial-sc-in-enter = Enter: confirm and save the edit. Escape: cancel and discard it.
tutorial-sc-in-backspace = Backspace: delete the character before the cursor. Ctrl+Backspace: delete the word before it, and say the word.
tutorial-sc-in-delete = Delete: delete the character after the cursor. Ctrl+Delete: delete the word after it.
tutorial-sc-in-newline = Ctrl+Enter: add a line break without leaving the edit.
tutorial-sc-in-move = Home and End: move the cursor to the start or end of the line, and say the line. Ctrl+Home and Ctrl+End: the start or end of the whole text. Ctrl+Left and Ctrl+Right: move a word at a time, and say the word. PgUp and PgDown: move it a screen of lines, and say the line it lands on.
tutorial-sc-in-select = Shift with an arrow key: extend a selection from the cursor, and Ctrl+Shift with Left or Right by a word. Shift+Home and Shift+End: select to the start or end of the line, Ctrl+Shift+Home and Ctrl+Shift+End of the whole text. Ctrl+A: select everything.
tutorial-sc-in-chain = Ctrl+Shift+I and Ctrl+Shift+A: save this edit and open a new item before or after it, staying in edit mode so you can keep typing.
tutorial-sc-command = Command and search
tutorial-sc-cmd-colon = : (colon): open command mode, type a command name and press Enter to run it.
tutorial-sc-cmd-f = F: search and filter the current list as you type. Enter goes to the match rather than activating it, and Escape clears the search.
tutorial-sc-cmd-manual = Tab: open the beginner's manual, a list that explains the basics from the top. Escape leaves it, and Tab opens it again.
tutorial-sc-cmd-ctrlf = Ctrl+F: extended search through all children, with results as a flat list you can jump to. It also works while you are editing text. Results from deeper down keep arriving while you type: the count says searching until the search is done, and a long search tells you when it ends.
tutorial-sc-cmd-lineends = Ctrl+Home and Ctrl+End: in a search or a command, go to the first or last item of the list. Home and End move the cursor in what you typed, and Ctrl+Left, Ctrl+Right and Ctrl+Backspace work by word there too.
tutorial-sc-cmd-scroll = S: scroll mode, flatten the list and its sublists into one continuous reading view.
tutorial-sc-cmd-history = z: open the read-only history view of this tab's undo timeline.
tutorial-sc-tabs = Tabs and window
tutorial-sc-tab-new = Ctrl+T: open a new tab. Ctrl+Shift+T: close the current tab, after you confirm.
tutorial-sc-tab-mru = Ctrl+Tab and Ctrl+Shift+Tab: step through tabs in most-recently-used order.
tutorial-sc-tab-number = Ctrl+1 through Ctrl+9: jump straight to a tab by its number.
tutorial-sc-tab-palette = t: open the tab switcher palette. Its first row, new tab, opens a new tab. Delete or Ctrl+D closes the highlighted tab.
tutorial-sc-tab-controls = c: open the window controls palette, minimize, maximize, or close the window.
tutorial-sc-files = Files, undo, and save
tutorial-sc-file-undo = Ctrl+Z: undo. Ctrl+Shift+Z: redo. Each tab keeps its own timeline.
tutorial-sc-file-clipboard = Ctrl+C: copy. Ctrl+X: cut. Ctrl+V: paste. Ctrl+Shift+C: copy the value behind the item, such as a link's address, instead of its label. Delete or Ctrl+D: delete the selected item.
tutorial-sc-file-save = Ctrl+S: save. Ctrl+Shift+S: save as. Ctrl+O: open a saved configuration file. Saving and opening a file need the file browser, from the Store.
tutorial-sc-file-update = Ctrl+U: install the updates that are ready, the app's and the programs'.

# ---------------------------------------------------------------------------
# 3. How it works (the mental model, lean)
# ---------------------------------------------------------------------------
tutorial-sec-how-it-works = How it works
tutorial-hiw-tree = Every graphical interface is really a hierarchy. Sicompass makes that hierarchy explicit and navigable entirely from the keyboard, so one set of keys drives everything.
tutorial-hiw-programs = Each item at the root is a program, also called a provider: the web browser, email, this tutorial. They all plug into the same tree, so once you can navigate one you can navigate them all.
tutorial-hiw-modes = Keys mean different things in different modes. You start in general mode and drop into insert, command, or search mode as needed. The Terminal and Claude views name themselves first command mode, because : is what opens them, and a second : opens second command mode, where you write a command or a prompt and Tab completes it. The Shortcuts at a glance section lists them all.
tutorial-hiw-editing = Any item that contains an input box is editable. Press i or a to edit it, Enter to save, Escape to cancel. The settings program makes values editable, and the file browser, from the Store, makes file names editable.
tutorial-hiw-undo = Each tab keeps its own undo timeline. Ctrl+Z walks back through your actions, including the route you navigated, and Ctrl+Shift+Z walks forward. Press z to see the history.
tutorial-hiw-undo-caveats = A few things cannot be undone: a terminal command that already ran, a directory delete larger than 4 MiB once the trash is emptied, and a posted chat message (undo redacts it, so recipients see that it was deleted).
tutorial-hiw-accessibility = Screen reader support is built in through AccessKit on Linux, macOS, and Windows, with nothing to configure. Each item is spoken in its own detected language, so a French line inside an English interface is read with French pronunciation.

# ---------------------------------------------------------------------------
# 4. The programs (one short leaf each)
# ---------------------------------------------------------------------------
tutorial-sec-programs = The programs
tutorial-prog-intro = Sicompass turns each data source into the same navigable tree. Every installed program is in the root list. Programs that do not ship with the app are installed from the Store and removed there too, and each one you have installed describes itself here, followed by the two that ship with the app.
tutorial-prog-none = No programs installed yet. Open store, then programs, to add one.
tutorial-prog-store = Store: adds, updates and removes programs. Open store, then programs, pick one, and press Enter on install. Each program says what it may reach before you install it, and installing is free. Under tiers are Sicompass Cloud, Sicompass Commercial, Sponsor and Support.
tutorial-prog-settings = Settings: always the last item at the root. It configures Sicompass and every program, and changes take effect immediately.

# ---------------------------------------------------------------------------
# 5. Interactive playground (hands-on element types)
# ---------------------------------------------------------------------------
tutorial-sec-playground = Interactive playground
tutorial-play-intro = A hands-on playground for the interactive element types Sicompass supports. Try each one as you go.
tutorial-play-marks = A right arrow at the start of a row means there is more inside, and your screen reader says arrow right. A dash means there is nothing inside, and it says dash. A left arrow before the name of the parent means Left goes back, and it says arrow left. After the mark, a letter says what kind of row it is: i an input, c a checkbox, R a group of choices and r a choice in it, b a button, l a link, p a picture and v a video. More letters say what a row holds, such as l when a checkbox holds a link.
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
tutorial-play-link = Press Enter to lazy-load this linked file: __FFON_JSON__
tutorial-play-in-element = In element mode, moving inside one row
tutorial-nav-landing = Some rows hold several images, links or buttons. When you land on one, your screen reader says how many of each. Press Down and listen.
tutorial-nav-links-how = Press Right on the row below to start element mode on its first link. Right and Left move between the links, and your screen reader names each one. Enter follows the one you are on, Escape leaves.
tutorial-nav-links = One __FFON_JSON__ two __FFON_JSON__
tutorial-nav-buttons-how = Press Right on the row below to do the same with buttons. Right again moves to the second one, Enter presses the one you are on, and Escape leaves.
tutorial-nav-buttons = <button>first</button>First button <button>second</button>Second button
tutorial-nav-button-first-pressed = You pressed the first button. It does nothing, it is here so you can practice.
tutorial-nav-button-second-pressed = You pressed the second button. It does nothing, it is here so you can practice.
tutorial-nav-lines-how = Press Right on the row below, which wraps over several lines. Down and Up go to the line below or above, and Right and Left still go one by one.
tutorial-nav-lines = Page one __FFON_JSON__ page two __FFON_JSON__ page three __FFON_JSON__ page four __FFON_JSON__ page five __FFON_JSON__ page six __FFON_JSON__
tutorial-nav-done = That is in element mode. Press Left to go back.
tutorial-play-style = Styled lists, with HTML and CSS
tutorial-style-intro = Any list can be styled with CSS, and its rows can hold HTML tags. Press Right on a section below to see one. Its last row shows the CSS that does it.
tutorial-style-warm = <div class="warm">A warm colour theme</div>
tutorial-style-warm-text = These rows take their colours from a stylesheet.
tutorial-style-warm-more = Move up and down. The selected row has its own colour too.
tutorial-style-cards = <div class="cards">Cards</div>
tutorial-style-cards-one = Padding, a border and rounded corners make each row a card.
tutorial-style-cards-two = The keys and your screen reader do not change. A card is still a row.
tutorial-style-headings = Headings and bold
tutorial-style-h1 = <h1>A big heading</h1>
tutorial-style-h2 = <h2>A smaller heading</h2>
tutorial-style-inline = <p>A row can have a <b>bold</b> word, or a <span class="hot">coloured</span> one.</p>
tutorial-style-headings-how = These come from HTML tags inside the row, such as h1, h2, b and span.
tutorial-style-classes = Done and urgent rows
tutorial-style-done = <p class="done">Shipped last week</p>
tutorial-style-urgent = <p class="urgent">Needs an answer today</p>
tutorial-style-focus = <div class="markers">The cursor and the list prefix</div>
tutorial-style-focus-text = This list has its own cursor colour. Move up and down.
tutorial-style-marker-text = The arrow or dash at the start of a row can have its own colour as well.
tutorial-style-wide = <div class="wide">Using the full width</div>
tutorial-style-wide-text = Most lists stop at 120 characters so that lines stay easy to read, but this one uses the whole width of the window, however wide that is, which you can see by making the window larger and watching this line run on until it reaches the edge.
tutorial-style-site = Press Enter to visit a small site: __SITE_JSON__
tutorial-style-yours = Put a file called user.css in the same folder as settings.json to style every list your own way. It is read when Sicompass starts.
tutorial-play-grid = A grid of photos
tutorial-grid-intro = Pictures can sit side by side in a grid, like an album. The row below holds six of them. Move down to it, and your screen reader says how many images it holds.
tutorial-grid-photos = <div class="photos"><img src="asset:tutorial/photo-1.jpg" alt="Photo 1"><img src="asset:tutorial/photo-2.jpg" alt="Photo 2"><img src="asset:tutorial/photo-3.jpg" alt="Photo 3"><img src="asset:tutorial/photo-4.jpg" alt="Photo 4"><img src="asset:tutorial/photo-5.jpg" alt="Photo 5"><img src="asset:tutorial/photo-6.jpg" alt="Photo 6"></div>
tutorial-grid-keys = Press Right to enter in element mode on that row. Left and Right go from picture to picture, and Up and Down go to the row of pictures above or below. Press Enter or d to see the one you are on at full size, then Escape to come back.
tutorial-grid-css = The rule for .photos says display: grid, grid-template-columns: repeat(3, 1fr), gap: 15px. This is a grid of three equal columns, with a gap between them. The pictures are made square with aspect-ratio and object-fit.
tutorial-grid-more = The same works with display flex, for pictures side by side that wrap. Your own albums can use any of this. The photo album from the Store shows your own pictures this way.
tutorial-play-video = Video
tutorial-video-intro = Videos play inside Sicompass, like pictures. The row below holds a short clip with sound. Move down to it, and your screen reader says there is one video.
tutorial-video-clip = <video src="asset:tutorial/clip.mp4">Test clip</video>
tutorial-video-keys = Press Enter or d to play it. Enter pauses and plays again, and Escape goes back. If it has ended, Enter plays it again. A row can hold several videos and pictures, and Left and Right move between them while one is open.
tutorial-play-scroll = Scroll mode flattens this list and its sublists into one reading view. Press S, then use PgUp, PgDown, Home, and End on the long passage below.
tutorial-play-lorem = __LOREM_IPSUM__

# ---------------------------------------------------------------------------
# 6. Settings and config
# ---------------------------------------------------------------------------
tutorial-sec-config = Settings and config
tutorial-cfg-file = All configuration lives in one file, ~/.config/sicompass/settings.json, organized by namespace with a section per program.
tutorial-cfg-logs = Sicompass writes a daily log file, sicompass.log, to a platform directory: ~/.local/state/sicompass/ on Linux, ~/Library/Logs/sicompass/ on macOS, and %LOCALAPPDATA%\sicompass\ on Windows. Set the RUST_LOG environment variable to also print logs to the terminal. Logs help when you report a problem.
tutorial-cfg-settings = The Settings program edits everything live: color scheme, display scaling, and a shoulder-surfing blank that hides the screen while the screen reader keeps working. Each program brings its own values too, such as URL history under web browser, which sets how many addresses are remembered. In a desicompass session, color scheme, language and the accessibility settings, the screen reader among them, are in the superkey's Settings instead, shared with the login screen. Line numbers under text editor numbers the lines of a file, absolute or relative.
tutorial-cfg-saveload = Some programs support their own config files. Ctrl+S saves, Ctrl+Shift+S saves under a new name, and Ctrl+O opens a saved file.
tutorial-cfg-updates = Sicompass checks for updates of itself and of your programs at launch, and says at the top which are ready. Press Ctrl+U to install them. A program update that asks for more access waits for you in the store, where you see what it asks for. Program updates are verified by their signature and SHA-256. Where Nix installed Sicompass, a new version is named but not installed: update your flake pin and rebuild. A program update that needs a newer Sicompass waits, and the line at the top says which version it needs.

# ---------------------------------------------------------------------------
# 7. Extending Sicompass (pointer to the real docs)
# ---------------------------------------------------------------------------
tutorial-sec-extending = Extending Sicompass
tutorial-ext-build = You can build your own programs. Rust is the standard way, implement the Provider trait from the sicompass-sdk crate and register it, exactly as every built-in program does. A plugin you install is a program of its own instead, which Sicompass starts and talks to. It runs with your rights, so its permissions say what it means to do, and you approve them before it runs. Install one under ~/.config/sicompass/plugins/ and approve it in the Store.
tutorial-ext-docs = The full Provider trait, the element tags, and a worked example are in the repository docs and the sicompass-sdk crate, not in this tutorial. See docs/process-plugins.md for how a plugin talks to Sicompass, the lib/ crates for built-in Rust providers, and the sales demo plugin (salesdemo-plugin-sicompass on GitHub) for a complete plugin.
