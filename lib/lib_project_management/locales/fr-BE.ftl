# Project-management provider strings — Français (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = gestion de projet

pm-empty-columns = aucune colonne pour l'instant, appuyez sur ctrl+a pour en ajouter une
pm-empty-cards = aucune carte pour l'instant, appuyez sur ctrl+a pour en ajouter une

pm-cmd-move-up = déplacer vers le haut
pm-cmd-move-down = déplacer vers le bas
pm-cmd-move-left = déplacer vers la colonne précédente
pm-cmd-move-right = déplacer vers la colonne suivante

pm-error-unreadable = votre tableau n'a pas pu être lu, rien n'a donc été enregistré, les fichiers sur le disque sont intacts
pm-error-save = votre tableau n'a pas pu être enregistré
pm-error-no-column = ajoutez d'abord une colonne, une carte doit vivre dans une colonne
pm-error-nothing-to-paste = rien n'a encore été copié

pm-board-empty-slot = appuyez sur ctrl+a pour une première carte
pm-board-no-columns = aucune colonne, ajoutez-en une dans la liste avec ctrl+a

pm-say-card = { $column }, carte { $index } sur { $total }, { $text }
pm-say-column-empty = colonne { $index } sur { $total }, { $title }, vide
pm-say-insert = mode insertion, { $text }
pm-say-insert-empty = mode insertion, vide
pm-say-board = mode tableau
pm-say-deleted = supprimé, { $text }
pm-say-copied = copié, { $text }
pm-say-cut = coupé, { $text }
pm-say-pasted = collé, { $text }
pm-say-undone = annulé, { $what }
pm-say-redone = rétabli, { $what }
pm-say-edge = pas plus loin

pm-op-add-card = ajouter une carte
pm-op-delete-card = supprimer une carte
pm-op-rename-card = renommer une carte
pm-op-move-card = déplacer une carte
