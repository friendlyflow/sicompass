# Keyboard-hint labels — English (source/fallback). Each entry preserves the
# chord prefix from the source SHORTCUTS table; non-English locales also
# keep the chord literal (keys names don't translate) and only swap the verb.

hint-esc-back = Esc    Back

hint-up-previous = Up     Previous
hint-up-scroll-up = Up     Scroll up

hint-down-next = Down   Next
hint-down-scroll-dn = Down   Scroll dn

hint-right-open = Right  Open
hint-right-navigate = Right  Navigate
hint-right-cursor-right = Right  Cursor right

hint-left-back = Left   Back
hint-left-navigate = Left   Navigate
hint-left-cursor-left = Left   Cursor left

hint-pgup-page-up = PgUp   Page up
hint-pgdn-page-dn = PgDn   Page dn

hint-home-first = Home   First
hint-end-last = End    Last

hint-shift-up-select-up = Shift+Up Select up
hint-shift-down-select-dn = Shift+Down Select dn
hint-shift-left-select-left = Shift+Left Select left
hint-shift-right-select-right = Shift+Right Select right
hint-shift-home-sel-start = Shift+Home Sel. start
hint-shift-end-sel-end = Shift+End  Sel. end

hint-ctrl-home-line-start = Ctrl+Home Line start
hint-ctrl-end-line-end = Ctrl+End  Line end

hint-enter-activate = Enter  Activate
hint-enter-confirm = Enter  Confirm
hint-enter-execute = Enter  Execute
hint-enter-append = Enter  Append
hint-enter-open = Enter  Open
hint-enter-follow-link = Enter  Follow link
hint-enter-go-to-element = Enter  Go to element

hint-tab-search = Tab    Search
hint-tab-prefix-search = Tab    Prefix search
hint-tab-next-field = Tab    Next field

hint-bspc-backspace = Bspc   Backspace
hint-del-delete = Del    Delete
hint-del-delete-fwd = Del    Delete fwd
hint-f5-refresh = F5     Refresh

hint-m-meta = M      Meta
hint-z-timeline = Z      Timeline
hint-w-where-am-i = w      Where am I
hint-i-edit = I      Edit
hint-i-edit-input = I      Edit input
hint-a-append = A      Append
hint-d-dashboard = D      Dashboard
hint-s-scroll = S      Scroll
hint-command = :      Command

hint-ctrl-a-insert-after = Ctrl+A Insert after
hint-ctrl-a-select-all = Ctrl+A Select all
hint-ctrl-c-copy = Ctrl+C Copy
hint-ctrl-shift-c-copy-url-value = Ctrl+Shift+C Copy URL/value
hint-ctrl-d-delete = Ctrl+D Delete
hint-ctrl-enter-newline = Ctrl+Enter Newline
hint-ctrl-f-extended-search = Ctrl+F Extended search
hint-ctrl-i-insert-before = Ctrl+I Insert before
hint-ctrl-o-open = Ctrl+O Open
hint-ctrl-s-save = Ctrl+S Save
hint-ctrl-shift-a-insert-after = Ctrl+Shift+A Insert after
hint-ctrl-shift-i-insert-before = Ctrl+Shift+I Insert before
hint-ctrl-shift-s-save-as = Ctrl+Shift+S Save as
hint-ctrl-shift-tab-prev-tab = Ctrl+Shift+Tab Prev tab
hint-ctrl-shift-z-redo = Ctrl+Shift+Z Redo
hint-ctrl-t-new-tab = Ctrl+T New tab
hint-ctrl-tab-next-tab = Ctrl+Tab Next tab
hint-t-switch-tab = t      Switch tab
hint-enter-switch = Enter  Switch
hint-ctrl-u-install-update = Ctrl+U Install update
hint-ctrl-v-paste = Ctrl+V Paste
hint-ctrl-w-close-tab = Ctrl+W Close tab
hint-ctrl-x-cut = Ctrl+X Cut
hint-ctrl-z-undo = Ctrl+Z Undo

# Coordinate (input-mode) display names — spoken by the screen reader on
# every mode switch and shown in the header status line + window title.
mode-general = general mode
mode-insert = insert mode
mode-normal = normal mode
mode-visual = visual mode
mode-search = search
mode-extended-search = extended search
mode-command = command
mode-scroll = scroll mode
mode-scroll-search = scroll search
mode-scroll-prefix-search = scroll prefix search
mode-input-search = input search
mode-dashboard = dashboard
mode-meta = meta
mode-timeline = timeline
mode-tab-switcher = tab switcher

# Screen-reader announcements composed at runtime. Parameters:
#   $idx, $total — 1-based position and tab count
#   $label       — translated provider name of the active tab
speak-tab-change = tab { $idx }/{ $total }: { $label }
hint-c-controls = c      Controls
