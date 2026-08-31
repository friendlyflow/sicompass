# Git-client provider strings — Belgian German.

gitclient-display-name = git-Client

gitclient-head-branch = { $repo } auf { $branch }
gitclient-head-detached = { $repo }, losgeloest bei { $oid }
gitclient-head-unborn = { $repo }, noch keine Commits
gitclient-head-unreadable = { $repo }, konnte nicht gelesen werden

gitclient-ahead = { $n } voraus
gitclient-behind = { $n } zurueck
gitclient-in-sync = aktuell
gitclient-no-upstream = kein Upstream-Branch

# Abschnittsnamen. Zugleich die Bezeichnungen, ueber die der Cursor nach einer
# Aktualisierung wiedergefunden wird, also ohne Zaehler oder Veraenderliches.
gitclient-section-changes = Aenderungen
gitclient-section-graph = Graph
gitclient-section-branches = Branches
gitclient-section-stashes = Stashes
gitclient-section-remotes = Remotes

# Das Leerzeichen am Ende ist Absicht: die getippte Nachricht folgt darauf.
gitclient-message-label = Nachricht:{" "}

gitclient-button-commit = committen
gitclient-button-amend = committen und den letzten aendern
gitclient-button-push = committen und pushen
gitclient-button-sync = committen und synchronisieren

gitclient-clean = Arbeitsverzeichnis sauber

gitclient-change-modified = geaendert
gitclient-change-added = hinzugefuegt
gitclient-change-deleted = geloescht
gitclient-change-renamed = umbenannt
gitclient-change-copied = kopiert
gitclient-change-typechanged = Typ geaendert
gitclient-change-untracked = unversioniert
gitclient-change-changed = veraendert
gitclient-renamed-from = von { $from }

gitclient-conflict = Konflikt
gitclient-conflict-both-modified = Konflikt, beide geaendert
gitclient-conflict-both-added = Konflikt, beide hinzugefuegt
gitclient-conflict-both-deleted = Konflikt, beide geloescht
gitclient-conflict-added-by-us = Konflikt, von uns hinzugefuegt
gitclient-conflict-added-by-them = Konflikt, von ihnen hinzugefuegt
gitclient-conflict-deleted-by-us = Konflikt, von uns geloescht
gitclient-conflict-deleted-by-them = Konflikt, von ihnen geloescht

gitclient-binary = Binaerdatei, kein Text anzuzeigen
gitclient-unreadable = Datei konnte nicht gelesen werden
gitclient-diff-truncated = { $n } weitere Zeilen, nicht angezeigt
gitclient-empty = hier ist nichts
gitclient-gone = das gibt es nicht mehr, F5 druecken

gitclient-error-not-a-repository = kein git-Repository

gitclient-no-commits = noch keine Commits
gitclient-load-more = { $n } weitere Commits laden
gitclient-stats = { $files } Dateien geaendert, { $ins } Zeilen hinzu, { $del } Zeilen entfernt
gitclient-commit-author = Autor: { $author }
gitclient-commit-date = Datum: { $date }
gitclient-commit-refs = Referenzen: { $refs }

gitclient-scope-local = lokal
gitclient-scope-remote = entfernt
gitclient-no-branches = keine Branches
gitclient-branch-current = der Branch, auf dem du bist
gitclient-branch-tracking = folgt { $upstream }
gitclient-branch-gone = der Upstream-Branch ist verschwunden
gitclient-branch-elsewhere = ausgecheckt in { $path }

gitclient-no-stashes = keine Stashes
gitclient-no-remotes = keine Remotes
gitclient-remote-fetch = Abrufen: { $url }
gitclient-remote-push = Senden: { $url }

gitclient-working = { $what } laeuft
gitclient-done = { $what } fertig
gitclient-stale = das Repository hat sich auf der Platte geaendert, F5 zum Neuladen

gitclient-confirm-cancel = abbrechen, nichts aendern
gitclient-confirm-yes = ja, und die Aenderungen an { $what } verwerfen

gitclient-error-locked = in diesem Repository laeuft bereits ein anderer git-Prozess, versuche es danach erneut
gitclient-error-select-a-file = setze den Cursor zuerst auf eine Datei
gitclient-error-select-a-commit = setze den Cursor zuerst auf einen Commit
gitclient-error-select-a-branch = setze den Cursor zuerst auf einen Branch
gitclient-error-select-a-stash = setze den Cursor zuerst auf einen Stash
gitclient-error-select-a-remote = setze den Cursor zuerst auf ein Remote
gitclient-error-not-staged = diese Datei ist nicht vorgemerkt
gitclient-error-remote-syntax = gib einen Namen und eine URL an, durch ein Leerzeichen getrennt
gitclient-error-no-message = tippe zuerst eine Nachricht, druecke i auf der Nachrichtenzeile
gitclient-error-conflicts = loese zuerst die Konflikte, merke jede Datei vor, sobald sie in Ordnung ist
gitclient-error-nothing-to-amend = es gibt noch keinen Commit zum Aendern
gitclient-error-busy = eine andere entfernte Operation laeuft noch
gitclient-error-undo = das konnte nicht rueckgaengig gemacht werden
