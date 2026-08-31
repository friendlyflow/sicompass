# Git-client provider strings — Belgian Dutch (Flemish).

gitclient-display-name = git client

gitclient-head-branch = { $repo } op { $branch }
gitclient-head-detached = { $repo }, losgekoppeld op { $oid }
gitclient-head-unborn = { $repo }, nog geen commits
gitclient-head-unreadable = { $repo }, kon niet gelezen worden

gitclient-ahead = { $n } voor
gitclient-behind = { $n } achter
gitclient-in-sync = bijgewerkt
gitclient-no-upstream = geen upstream tak

# Sectienamen. Ook de labels waarmee de cursor na een verversing teruggezet
# wordt, dus zonder aantallen of iets anders dat verandert.
gitclient-section-changes = wijzigingen
gitclient-section-graph = grafiek
gitclient-section-branches = takken
gitclient-section-stashes = opzijgezet
gitclient-section-remotes = remotes

# De spatie op het einde is bedoeld: het getypte bericht volgt erop.
gitclient-message-label = bericht:{" "}

gitclient-button-commit = vastleggen
gitclient-button-amend = vastleggen, de vorige aanpassen
gitclient-button-push = vastleggen en versturen
gitclient-button-sync = vastleggen en synchroniseren

gitclient-clean = werkmap is schoon

gitclient-change-modified = gewijzigd
gitclient-change-added = toegevoegd
gitclient-change-deleted = verwijderd
gitclient-change-renamed = hernoemd
gitclient-change-copied = gekopieerd
gitclient-change-typechanged = type gewijzigd
gitclient-change-untracked = niet gevolgd
gitclient-change-changed = veranderd
gitclient-renamed-from = van { $from }

gitclient-conflict = conflict
gitclient-conflict-both-modified = conflict, beide gewijzigd
gitclient-conflict-both-added = conflict, beide toegevoegd
gitclient-conflict-both-deleted = conflict, beide verwijderd
gitclient-conflict-added-by-us = conflict, door ons toegevoegd
gitclient-conflict-added-by-them = conflict, door hen toegevoegd
gitclient-conflict-deleted-by-us = conflict, door ons verwijderd
gitclient-conflict-deleted-by-them = conflict, door hen verwijderd

gitclient-binary = binair bestand, geen tekst om te tonen
gitclient-unreadable = bestand kon niet gelezen worden
gitclient-diff-truncated = { $n } regels meer, niet getoond
gitclient-empty = hier staat niets
gitclient-gone = dit bestaat niet meer, druk F5

gitclient-error-not-a-repository = geen git repository

gitclient-no-commits = nog geen commits
gitclient-load-more = { $n } commits meer laden
gitclient-stats = { $files } bestanden gewijzigd, { $ins } regels bij, { $del } regels weg
gitclient-commit-author = auteur: { $author }
gitclient-commit-date = datum: { $date }
gitclient-commit-refs = verwijzingen: { $refs }

gitclient-scope-local = lokaal
gitclient-scope-remote = extern
gitclient-no-branches = geen takken
gitclient-branch-current = de tak waar je op staat
gitclient-branch-tracking = volgt { $upstream }
gitclient-branch-gone = de upstream tak bestaat niet meer
gitclient-branch-elsewhere = uitgecheckt in { $path }

gitclient-no-stashes = niets opzijgezet
gitclient-no-remotes = geen remotes
gitclient-remote-fetch = ophalen: { $url }
gitclient-remote-push = versturen: { $url }

gitclient-working = { $what } bezig
gitclient-done = { $what } klaar
gitclient-stale = de repository is op schijf veranderd, druk F5 om te herladen

gitclient-confirm-cancel = annuleren, niets wijzigen
gitclient-confirm-yes = ja, en de wijzigingen aan { $what } weggooien

gitclient-error-locked = er loopt al een ander git proces in deze repository, probeer opnieuw als het klaar is
gitclient-error-select-a-file = zet de cursor eerst op een bestand
gitclient-error-select-a-commit = zet de cursor eerst op een commit
gitclient-error-select-a-branch = zet de cursor eerst op een tak
gitclient-error-select-a-stash = zet de cursor eerst op iets dat opzijgezet is
gitclient-error-select-a-remote = zet de cursor eerst op een remote
gitclient-error-not-staged = dat bestand staat niet klaar om vast te leggen
gitclient-error-remote-syntax = typ een naam en een url, gescheiden door een spatie
gitclient-error-no-message = typ eerst een bericht, druk i op de berichtrij
gitclient-error-conflicts = los eerst de conflicten op, zet elk bestand klaar zodra het in orde is
gitclient-error-nothing-to-amend = er is nog geen commit om aan te passen
gitclient-error-busy = er loopt nog een andere externe bewerking
gitclient-error-undo = dat kon niet ongedaan gemaakt worden
