# File-browser provider strings — English (source/fallback).

filebrowser-display-name = file browser

# Error messages. Dynamic substitutions use named Fluent parameters so
# translators can rearrange them in their own grammar.
filebrowser-error-redo-delete-trash-failed = redo delete: trash failed: { $err }
filebrowser-error-open-with-not-file = open with: select a file, not a directory
filebrowser-error-open-with-no-filename = open with: could not extract filename
filebrowser-error-create-file = could not create { $name }: { $err }
filebrowser-error-create-directory = could not create folder { $name }: { $err }
filebrowser-error-delete = could not delete { $name }: { $err }
filebrowser-error-rename = could not rename { $old } to { $new }: { $err }
filebrowser-error-copy = could not paste { $name }: { $err }
filebrowser-error-add-here = cannot add to { $dir }: { $err }
# The { $err } of the messages above when the system refused for lack of rights.
filebrowser-error-reason-permission-denied = permission denied

filebrowser-description = Your files and folders as a list of lists: browse, rename, create, copy, move and delete, all undoable.
filebrowser-radio-sort-order = sort order

# The tutorial's paragraphs about this program, under its programs section:
# <name>-tutorial, then <name>-tutorial-2 and so on, read until one is missing.
filebrowser-tutorial = File browser, from the Store: your filesystem as a tree. Enter directories with Right, rename with i, and create, copy, paste, or delete items inline. Dot-prefixed files are hidden, run the colon command show/hide hidden files to see them.
