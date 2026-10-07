# Project-management provider strings — Nederlands (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = projectbeheer

projectmanagement-empty-columns = nog geen kolommen, druk ctrl+a om er een toe te voegen
projectmanagement-empty-cards = nog geen kaarten, druk ctrl+a om er een toe te voegen

projectmanagement-list-meta = lijstinfo:
projectmanagement-sha256 = sha256: { $hash }
projectmanagement-error-meta-undeletable = de lijstinfo hoort bij de lijst en kan niet verwijderd worden
projectmanagement-error-meta-readonly = de lijstinfo kan niet bewerkt worden

projectmanagement-cmd-move-up = omhoog verplaatsen
projectmanagement-cmd-move-down = omlaag verplaatsen
projectmanagement-cmd-move-left = naar vorige kolom verplaatsen
projectmanagement-cmd-move-right = naar volgende kolom verplaatsen
projectmanagement-cmd-archive-card = kaart archiveren

# The settings checkbox that turns on mirroring the board to the server.
projectmanagement-checkbox-cloud-backup = cloudsynchronisatie inschakelen

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

projectmanagement-description = Een kanbanbord dat je in een lijst of op een raster ordent, met een optionele cloudsynchronisatie.
projectmanagement-service = houdt je bord gelijk tussen je computers via de server van Sicompass Cloud

projectmanagement-cloud-needs-payment = cloudsynchronisatie: heeft Sicompass Cloud nodig, zie store, abonnementen
projectmanagement-cloud-active = cloudsynchronisatie: aan, verlengt over { $days } dagen
projectmanagement-cloud-grace = cloudsynchronisatie: abonnement verlopen, nog { $days } dagen aan, vernieuw in store, abonnementen
projectmanagement-cloud-expired = cloudsynchronisatie: uit, het abonnement is { $days } dagen geleden verlopen, zie store, abonnementen
projectmanagement-cloud-needs-subscription = cloudsynchronisatie heeft Sicompass Cloud nodig, zie store, abonnementen
projectmanagement-cloud-failed = cloudsynchronisatie mislukt: { $reason }
projectmanagement-error-cloud-row-undeletable = de rij van de cloudsynchronisatie is geen kolom, zet cloudsynchronisatie uit in de instellingen om ze weg te halen

projectmanagement-cmd-sync-now = nu synchroniseren met de cloud
projectmanagement-sync-pulled = wijzigingen van je andere computers zijn toegevoegd
projectmanagement-sync-conflicts = wijzigingen van je andere computers zijn toegevoegd, { $count } aan beide kanten bewerkt, de laatste bewerking is behouden
projectmanagement-sync-status-synced = cloud: gesynchroniseerd
projectmanagement-sync-status-changed = cloud: gewijzigd sinds de laatste synchronisatie
projectmanagement-sync-status-new = cloud: nog niet gesynchroniseerd

projectmanagement-tutorial = Projectbeheer, uit de store: een kanbanbord. Druk op ctrl+a om een kolom toe te voegen, Rechts om ze te openen en ctrl+a erin om een kaart toe te voegen. Een kolom bevat kaarten en niets dieper, dus een kaart is het laatste niveau. De eerste rij van elke lijst is de lijstinfo, met de hash van de lijst en, met cloudsynchronisatie aan, of ze gewijzigd is sinds de laatste synchronisatie.
projectmanagement-tutorial-2 = d: open het bord zelf, met de kolommen naast elkaar. De cursor staat op een kaart, en enkel die kaart licht op. Links en Rechts gaan tussen kolommen, Omhoog en Omlaag door de kaarten van een kolom. i en a bewerken de kaart, o en shift+o openen een nieuwe eronder of erboven, ctrl+d verwijdert, en ctrl+x, ctrl+c en ctrl+v knippen, kopiëren en plakken. Een lege kolom toont een plek waar u kan staan om de eerste kaart toe te voegen. De kolommen zelf beheert u in de lijst. Escape gaat terug.
