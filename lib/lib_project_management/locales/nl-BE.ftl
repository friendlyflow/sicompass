# Project-management provider strings — Nederlands (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = projectbeheer

pm-empty-columns = nog geen kolommen, druk ctrl+a om er een toe te voegen
pm-empty-cards = nog geen kaarten, druk ctrl+a om er een toe te voegen

pm-cmd-move-up = omhoog verplaatsen
pm-cmd-move-down = omlaag verplaatsen
pm-cmd-move-left = naar vorige kolom verplaatsen
pm-cmd-move-right = naar volgende kolom verplaatsen
pm-cmd-archive-card = kaart archiveren

pm-error-unreadable = je bord kon niet gelezen worden, er is dus niets opgeslagen, de bestanden op schijf blijven ongewijzigd
pm-error-save = je bord kon niet opgeslagen worden
pm-error-no-column = voeg eerst een kolom toe, een kaart hoort in een kolom
pm-error-nothing-to-paste = er is nog niets gekopieerd

pm-board-empty-slot = druk ctrl+a voor een eerste kaart
pm-board-no-columns = nog geen kolommen, voeg er een toe in de lijst met ctrl+a

# De titel van de archiefkolom bij het aanmaken. Enkel een naam: de kolom
# wordt op id herkend, dus hernoemen verandert daar niets aan.
pm-archive-title = Archief

pm-say-card = { $column }, kaart { $index } van { $total }, { $text }
pm-say-column-empty = kolom { $index } van { $total }, { $title }, leeg
pm-say-insert = invoegmodus, { $text }
pm-say-insert-empty = invoegmodus, leeg
pm-say-board = bordmodus
pm-say-deleted = verwijderd, { $text }
pm-say-copied = gekopieerd, { $text }
pm-say-cut = geknipt, { $text }
pm-say-pasted = geplakt, { $text }
pm-say-archived = gearchiveerd, { $text }
pm-say-nothing-to-archive = niets om te archiveren, zet de cursor eerst op een kaart
pm-say-already-archived = al gearchiveerd
pm-say-undone = ongedaan gemaakt, { $what }
pm-say-redone = opnieuw gedaan, { $what }
pm-say-edge = niet verder

pm-op-add-card = kaart toevoegen
pm-op-delete-card = kaart verwijderen
pm-op-rename-card = kaart hernoemen
pm-op-move-card = kaart verplaatsen
pm-op-archive-card = kaart archiveren
