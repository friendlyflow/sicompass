# Text-editor provider strings — Belgian German (Eastern Cantons).

texteditor-display-name = Texteditor

# Fehlermeldungen
texteditor-error-undo-delete-line-write-failed = Zeile löschen rückgängig: Datei schreiben fehlgeschlagen
texteditor-error-undo-insert-line-write-failed = Zeile einfügen rückgängig: Datei schreiben fehlgeschlagen
texteditor-error-redo-delete-line-write-failed = Zeile löschen wiederholen: Datei schreiben fehlgeschlagen
texteditor-error-redo-insert-line-write-failed = Zeile einfügen wiederholen: Datei schreiben fehlgeschlagen
texteditor-error-redo-delete-trash-failed = Löschen wiederholen: Papierkorb fehlgeschlagen: { $err }
texteditor-error-create-file = { $name } konnte nicht erstellt werden: { $err }
texteditor-error-create-directory = Ordner { $name } konnte nicht erstellt werden: { $err }
texteditor-error-delete = { $name } konnte nicht gelöscht werden: { $err }
texteditor-error-rename = { $old } konnte nicht in { $new } umbenannt werden: { $err }
texteditor-error-save = { $name } konnte nicht gespeichert werden: { $err }
texteditor-error-add-here = in { $path } kann nichts hinzugefügt werden: { $err }
texteditor-error-change-here = { $path } kann nicht geändert werden: { $err }
texteditor-error-reason-permission-denied = Zugriff verweigert

# A .ff file: a tree of nodes, each hashed over its children and its text.
# Every level of a .ff file opens with this row. Inside it: the hash of the
# node whose children the level shows.
texteditor-ff-header = Kopfzeile
texteditor-ff-sha256 = sha256: { $hash }
texteditor-ff-invalid = (keine gültige .ff-Datei: { $err })
texteditor-ff-reason-truncated = sie endet mitten in einem Knoten
texteditor-ff-reason-trailing = nach ihrem Wurzelknoten folgen noch Bytes
texteditor-ff-reason-utf8 = der Text eines Knotens ist kein UTF-8
texteditor-ff-reason-too-deep = ihre Knoten sind mehr als 256 Ebenen tief verschachtelt
texteditor-ff-reason-no-node = dieser Knoten existiert nicht mehr
texteditor-error-ff-hash = { $name } wurde außerhalb von sicompass geändert oder ist beschädigt, die Hashes stimmen nicht. Beim Speichern werden neue geschrieben.
texteditor-error-ff-too-long = ein Knoten enthält höchstens 255 Bytes Text
texteditor-error-ff-too-many = ein Knoten hat höchstens 255 Kinder
texteditor-error-undo-ff-write-failed = Rückgängig: die .ff-Datei konnte nicht geschrieben werden
texteditor-error-redo-ff-write-failed = Wiederholen: die .ff-Datei konnte nicht geschrieben werden

texteditor-description = Bearbeiten Sie jede Textdatei als Liste ihrer Zeilen, mit Einrückung und Abschnitten als Ebenen, und jede Änderung rückgängig zu machen.
texteditor-setting-path = Pfad des Texteditors

texteditor-tutorial = Texteditor, aus dem Store: drücken Sie Rechts auf einer Datei, um ihren Inhalt als Baum zu öffnen und die Zeilen vor Ort zu bearbeiten. Jede Änderung steht auf der Undo-Zeitleiste.
