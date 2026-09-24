# Project-management provider strings — Nederlands (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = projectbeheer

projectmanagement-empty-columns = nog geen kolommen, druk ctrl+a om er een toe te voegen
projectmanagement-empty-cards = nog geen kaarten, druk ctrl+a om er een toe te voegen

projectmanagement-cmd-move-up = omhoog verplaatsen
projectmanagement-cmd-move-down = omlaag verplaatsen
projectmanagement-cmd-move-left = naar vorige kolom verplaatsen
projectmanagement-cmd-move-right = naar volgende kolom verplaatsen
projectmanagement-cmd-archive-card = kaart archiveren

# The settings checkbox that turns on mirroring the board to the server.
projectmanagement-checkbox-cloud-backup = cloudback-up inschakelen

projectmanagement-error-unreadable = je bord kon niet gelezen worden, er is dus niets opgeslagen, de bestanden op schijf blijven ongewijzigd
projectmanagement-error-save = je bord kon niet opgeslagen worden
projectmanagement-error-no-column = voeg eerst een kolom toe, een kaart hoort in een kolom
projectmanagement-error-nothing-to-paste = er is nog niets gekopieerd

projectmanagement-board-empty-slot = druk ctrl+a voor een eerste kaart
projectmanagement-board-no-columns = nog geen kolommen, voeg er een toe in de lijst met ctrl+a

# De titel van de archiefkolom bij het aanmaken. Enkel een naam: de kolom
# wordt op id herkend, dus hernoemen verandert daar niets aan.
projectmanagement-archive-title = Archief

projectmanagement-say-card = { $column }, kaart { $index } van { $total }, { $text }
projectmanagement-say-column-empty = kolom { $index } van { $total }, { $title }, leeg
projectmanagement-say-insert = invoegmodus, { $text }
projectmanagement-say-insert-empty = invoegmodus, leeg
projectmanagement-say-board = bordmodus
projectmanagement-say-deleted = verwijderd, { $text }
projectmanagement-say-copied = gekopieerd, { $text }
projectmanagement-say-cut = geknipt, { $text }
projectmanagement-say-pasted = geplakt, { $text }
projectmanagement-say-archived = gearchiveerd, { $text }
projectmanagement-say-nothing-to-archive = niets om te archiveren, zet de cursor eerst op een kaart
projectmanagement-say-already-archived = al gearchiveerd
projectmanagement-say-undone = ongedaan gemaakt, { $what }
projectmanagement-say-redone = opnieuw gedaan, { $what }
projectmanagement-say-edge = niet verder

projectmanagement-op-add-card = kaart toevoegen
projectmanagement-op-delete-card = kaart verwijderen
projectmanagement-op-rename-card = kaart hernoemen
projectmanagement-op-move-card = kaart verplaatsen
projectmanagement-op-archive-card = kaart archiveren

projectmanagement-description = Een kanbanbord dat je in een lijst of op een raster ordent, met een optionele cloudback-up.
projectmanagement-service = bewaart een kopie van je bord op de server van Sicompass Cloud

projectmanagement-cloud-needs-payment = cloudback-up: heeft Sicompass Cloud nodig, zie store, abonnementen
projectmanagement-cloud-active = cloudback-up: aan, verlengt over { $days } dagen
projectmanagement-cloud-grace = cloudback-up: abonnement verlopen, nog { $days } dagen aan, vernieuw in store, abonnementen
projectmanagement-cloud-expired = cloudback-up: uit, het abonnement is { $days } dagen geleden verlopen, zie store, abonnementen
projectmanagement-cloud-needs-subscription = cloudback-up heeft Sicompass Cloud nodig, zie store, abonnementen
projectmanagement-cloud-failed = cloudback-up mislukt: { $reason }
projectmanagement-error-cloud-row-undeletable = de rij van de cloudback-up is geen kolom, zet cloudback-up uit in de instellingen om ze weg te halen

projectmanagement-cmd-restore-backup = cloudback-up terugzetten
projectmanagement-restore-done = cloudback-up teruggezet
projectmanagement-restore-empty = er is geen cloudback-up om terug te zetten
projectmanagement-restore-refused = je bord is niet leeg, er werd niets teruggezet
