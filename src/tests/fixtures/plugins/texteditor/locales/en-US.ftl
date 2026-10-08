# Text-editor provider strings — English (source/fallback).

texteditor-display-name = text editor

# Error messages
texteditor-error-undo-delete-line-write-failed = undo delete line: failed to write file
texteditor-error-undo-insert-line-write-failed = undo insert line: failed to write file
texteditor-error-redo-delete-line-write-failed = redo delete line: failed to write file
texteditor-error-redo-insert-line-write-failed = redo insert line: failed to write file
texteditor-error-redo-delete-trash-failed = redo delete: trash failed: { $err }
texteditor-error-create-file = could not create { $name }: { $err }
texteditor-error-create-directory = could not create folder { $name }: { $err }
texteditor-error-delete = could not delete { $name }: { $err }
texteditor-error-rename = could not rename { $old } to { $new }: { $err }
texteditor-error-save = could not save { $name }: { $err }
texteditor-error-add-here = cannot add to { $path }: { $err }
texteditor-error-change-here = cannot change { $path }: { $err }
# The { $err } of the messages above when the system refused for lack of rights.
texteditor-error-reason-permission-denied = permission denied

texteditor-description = Edit any text file as a list of its lines, with indentation and sections as levels, and every change undoable.
texteditor-setting-path = text editor path

# The tutorial's paragraphs about this program, under its programs section:
# <name>-tutorial, then <name>-tutorial-2 and so on, read until one is missing.
texteditor-tutorial = Text editor, from the Store: press Right on a file to open its contents as a tree and edit the lines inline. Every change is on the undo timeline.
