# Project-management provider strings — Français (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = gestion de projet

projectmanagement-empty-columns = aucune colonne pour l'instant, appuyez sur ctrl+a pour en ajouter une
projectmanagement-empty-cards = aucune carte pour l'instant, appuyez sur ctrl+a pour en ajouter une

projectmanagement-cmd-move-up = déplacer vers le haut
projectmanagement-cmd-move-down = déplacer vers le bas
projectmanagement-cmd-move-left = déplacer vers la colonne précédente
projectmanagement-cmd-move-right = déplacer vers la colonne suivante
projectmanagement-cmd-archive-card = archiver la carte

# The settings checkbox that turns on mirroring the board to the server.
projectmanagement-checkbox-cloud-backup = activer la sauvegarde cloud

projectmanagement-error-unreadable = votre tableau n'a pas pu être lu, rien n'a donc été enregistré, les fichiers sur le disque sont intacts
projectmanagement-error-save = votre tableau n'a pas pu être enregistré
projectmanagement-error-no-column = ajoutez d'abord une colonne, une carte doit vivre dans une colonne
projectmanagement-error-nothing-to-paste = rien n'a encore été copié

projectmanagement-board-empty-slot = appuyez sur ctrl+a pour une première carte
projectmanagement-board-no-columns = aucune colonne, ajoutez-en une dans la liste avec ctrl+a

# Le titre de la colonne d'archives à sa création. Un nom seulement : la
# colonne est identifiée par son id, la renommer n'y change rien.
projectmanagement-archive-title = Archives

projectmanagement-say-card = { $column }, carte { $index } sur { $total }, { $text }
projectmanagement-say-column-empty = colonne { $index } sur { $total }, { $title }, vide
projectmanagement-say-insert = mode insertion, { $text }
projectmanagement-say-insert-empty = mode insertion, vide
projectmanagement-say-board = mode tableau
projectmanagement-say-deleted = supprimé, { $text }
projectmanagement-say-copied = copié, { $text }
projectmanagement-say-cut = coupé, { $text }
projectmanagement-say-pasted = collé, { $text }
projectmanagement-say-archived = archivé, { $text }
projectmanagement-say-nothing-to-archive = rien à archiver, placez d'abord le curseur sur une carte
projectmanagement-say-already-archived = déjà archivé
projectmanagement-say-undone = annulé, { $what }
projectmanagement-say-redone = rétabli, { $what }
projectmanagement-say-edge = pas plus loin

projectmanagement-op-add-card = ajouter une carte
projectmanagement-op-delete-card = supprimer une carte
projectmanagement-op-rename-card = renommer une carte
projectmanagement-op-move-card = déplacer une carte
projectmanagement-op-archive-card = archiver une carte

projectmanagement-description = Un tableau kanban que vous organisez en liste ou en grille, avec une sauvegarde cloud en option.
projectmanagement-service = garde une copie de votre tableau sur le serveur de Sicompass Cloud

projectmanagement-cloud-needs-payment = sauvegarde cloud : demande Sicompass Cloud, voir store, abonnements
projectmanagement-cloud-active = sauvegarde cloud : active, renouvellement dans { $days } jours
projectmanagement-cloud-grace = sauvegarde cloud : abonnement expiré, encore active { $days } jours, renouvelez dans store, abonnements
projectmanagement-cloud-expired = sauvegarde cloud : arrêtée, l'abonnement a expiré il y a { $days } jours, voir store, abonnements
projectmanagement-cloud-needs-subscription = la sauvegarde cloud demande Sicompass Cloud, voir store, abonnements
projectmanagement-cloud-failed = échec de la sauvegarde cloud : { $reason }
projectmanagement-error-cloud-row-undeletable = la ligne de la sauvegarde cloud n'est pas une colonne, désactivez la sauvegarde cloud dans les paramètres pour la retirer

projectmanagement-cmd-restore-backup = restaurer la sauvegarde cloud
projectmanagement-restore-done = sauvegarde cloud restaurée
projectmanagement-restore-empty = il n'y a aucune sauvegarde cloud à restaurer
projectmanagement-restore-refused = votre tableau n'est pas vide, rien n'a été restauré
