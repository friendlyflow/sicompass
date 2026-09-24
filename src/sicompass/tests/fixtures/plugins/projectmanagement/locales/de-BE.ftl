# Project-management provider strings — Deutsch (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = projektverwaltung

projectmanagement-empty-columns = noch keine spalten, drücken sie strg+a um eine hinzuzufügen
projectmanagement-empty-cards = noch keine karten, drücken sie strg+a um eine hinzuzufügen

projectmanagement-cmd-move-up = nach oben verschieben
projectmanagement-cmd-move-down = nach unten verschieben
projectmanagement-cmd-move-left = in die vorige spalte verschieben
projectmanagement-cmd-move-right = in die nächste spalte verschieben
projectmanagement-cmd-archive-card = karte archivieren

# The settings checkbox that turns on mirroring the board to the server.
projectmanagement-checkbox-cloud-backup = Cloud-Sicherung aktivieren

projectmanagement-error-unreadable = ihr board konnte nicht gelesen werden, daher wurde nichts gespeichert, die dateien auf der festplatte bleiben unberührt
projectmanagement-error-save = ihr board konnte nicht gespeichert werden
projectmanagement-error-no-column = fügen sie zuerst eine spalte hinzu, eine karte gehört in eine spalte
projectmanagement-error-nothing-to-paste = es wurde noch nichts kopiert

projectmanagement-board-empty-slot = drücken Sie strg+a für eine erste karte
projectmanagement-board-no-columns = noch keine spalten, fügen Sie eine in der liste mit strg+a hinzu

# Der Titel der Archivspalte bei ihrer Erstellung. Nur ein Name: die Spalte
# wird über ihre id erkannt, Umbenennen ändert daran nichts.
projectmanagement-archive-title = Archiv

projectmanagement-say-card = { $column }, karte { $index } von { $total }, { $text }
projectmanagement-say-column-empty = spalte { $index } von { $total }, { $title }, leer
projectmanagement-say-insert = einfügemodus, { $text }
projectmanagement-say-insert-empty = einfügemodus, leer
projectmanagement-say-board = boardmodus
projectmanagement-say-deleted = gelöscht, { $text }
projectmanagement-say-copied = kopiert, { $text }
projectmanagement-say-cut = ausgeschnitten, { $text }
projectmanagement-say-pasted = eingefügt, { $text }
projectmanagement-say-archived = archiviert, { $text }
projectmanagement-say-nothing-to-archive = nichts zu archivieren, setzen Sie den cursor zuerst auf eine karte
projectmanagement-say-already-archived = bereits archiviert
projectmanagement-say-undone = rückgängig gemacht, { $what }
projectmanagement-say-redone = wiederhergestellt, { $what }
projectmanagement-say-edge = nicht weiter

projectmanagement-op-add-card = karte hinzufügen
projectmanagement-op-delete-card = karte löschen
projectmanagement-op-rename-card = karte umbenennen
projectmanagement-op-move-card = karte verschieben
projectmanagement-op-archive-card = karte archivieren

projectmanagement-description = Ein Kanban-Board, das Sie in einer Liste oder auf einem Raster ordnen, mit optionaler Cloud-Sicherung.
projectmanagement-service = bewahrt eine Kopie Ihres Boards auf dem Server von Sicompass Cloud auf

projectmanagement-cloud-needs-payment = Cloud-Sicherung: braucht Sicompass Cloud, siehe store, Abos
projectmanagement-cloud-active = Cloud-Sicherung: an, verlängert sich in { $days } Tagen
projectmanagement-cloud-grace = Cloud-Sicherung: Abo abgelaufen, noch { $days } Tage an, verlängern Sie in store, Abos
projectmanagement-cloud-expired = Cloud-Sicherung: aus, das Abo ist vor { $days } Tagen abgelaufen, siehe store, Abos
projectmanagement-cloud-needs-subscription = die Cloud-Sicherung braucht Sicompass Cloud, siehe store, Abos
projectmanagement-cloud-failed = Cloud-Sicherung fehlgeschlagen: { $reason }
projectmanagement-error-cloud-row-undeletable = die Zeile der Cloud-Sicherung ist keine Spalte, schalten Sie die Cloud-Sicherung in den Einstellungen aus, um sie zu entfernen

projectmanagement-cmd-restore-backup = Cloud-Sicherung wiederherstellen
projectmanagement-restore-done = Cloud-Sicherung wiederhergestellt
projectmanagement-restore-empty = es gibt keine Cloud-Sicherung zum Wiederherstellen
projectmanagement-restore-refused = Ihr Board ist nicht leer, es wurde nichts wiederhergestellt
