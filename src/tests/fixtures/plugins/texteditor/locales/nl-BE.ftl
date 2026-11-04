# Text-editor provider strings — Belgian Dutch (Flemish).

texteditor-display-name = tekstverwerker

# Foutmeldingen
texteditor-error-undo-delete-line-write-failed = undo verwijder regel: bestand schrijven mislukt
texteditor-error-undo-insert-line-write-failed = undo invoeg regel: bestand schrijven mislukt
texteditor-error-redo-delete-line-write-failed = redo verwijder regel: bestand schrijven mislukt
texteditor-error-redo-insert-line-write-failed = redo invoeg regel: bestand schrijven mislukt
texteditor-error-redo-delete-trash-failed = redo verwijderen: prullenbak mislukt: { $err }
texteditor-error-create-file = kon { $name } niet aanmaken: { $err }
texteditor-error-create-directory = kon map { $name } niet aanmaken: { $err }
texteditor-error-delete = kon { $name } niet verwijderen: { $err }
texteditor-error-rename = kon { $old } niet hernoemen naar { $new }: { $err }
texteditor-error-save = kon { $name } niet opslaan: { $err }
texteditor-error-add-here = kan niets toevoegen aan { $path }: { $err }
texteditor-error-change-here = kan { $path } niet wijzigen: { $err }
texteditor-error-reason-permission-denied = geen toestemming

# A .ff file: a tree of nodes, each hashed over its children and its text.
# Every level of a .ff file opens with this row. Inside it: the hash of the
# node whose children the level shows.
texteditor-ff-header = kop
texteditor-ff-sha256 = sha256: { $hash }
texteditor-ff-invalid = (geen geldig .ff-bestand: { $err })
texteditor-ff-reason-truncated = het stopt midden in een knoop
texteditor-ff-reason-trailing = er staan bytes na de wortelknoop
texteditor-ff-reason-utf8 = de tekst van een knoop is geen UTF-8
texteditor-ff-reason-too-deep = de knopen zijn meer dan 256 niveaus diep genest
texteditor-ff-reason-no-node = die knoop is er niet meer
texteditor-error-ff-hash = { $name } is buiten sicompass gewijzigd of beschadigd, de hashes kloppen niet. Opslaan schrijft nieuwe.
texteditor-error-ff-too-long = een knoop bevat hoogstens 255 bytes tekst
texteditor-error-ff-too-many = een knoop heeft hoogstens 255 kinderen
texteditor-error-undo-ff-write-failed = ongedaan maken: kon het .ff-bestand niet schrijven
texteditor-error-redo-ff-write-failed = opnieuw: kon het .ff-bestand niet schrijven

texteditor-description = Bewerk elk tekstbestand als een lijst van zijn regels, met inspringing en secties als niveaus, en elke wijziging ongedaan te maken.
texteditor-setting-path = pad van de teksteditor

texteditor-tutorial = Tekstverwerker, uit de store: druk op Rechts op een bestand om de inhoud als een boom te openen en de regels ter plaatse te bewerken. Elke wijziging staat op de undo-tijdlijn.
