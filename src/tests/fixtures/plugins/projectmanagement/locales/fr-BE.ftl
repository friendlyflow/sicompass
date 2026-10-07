# Project-management provider strings — Français (BE).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = gestion de projet

projectmanagement-empty-columns = aucune colonne pour l'instant, appuyez sur ctrl+a pour en ajouter une
projectmanagement-empty-cards = aucune carte pour l'instant, appuyez sur ctrl+a pour en ajouter une

projectmanagement-list-meta = info de liste :
projectmanagement-sha256 = sha256 : { $hash }
projectmanagement-error-meta-undeletable = l'info de liste appartient à la liste et ne peut pas être supprimée
projectmanagement-error-meta-readonly = l'info de liste ne peut pas être modifiée

projectmanagement-cmd-move-up = déplacer vers le haut
projectmanagement-cmd-move-down = déplacer vers le bas
projectmanagement-cmd-move-left = déplacer vers la colonne précédente
projectmanagement-cmd-move-right = déplacer vers la colonne suivante
projectmanagement-cmd-archive-card = archiver la carte

# The settings checkbox that turns on mirroring the board to the server.
projectmanagement-checkbox-cloud-backup = activer la synchronisation cloud

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

projectmanagement-description = Un tableau kanban que vous organisez en liste ou en grille, avec une synchronisation cloud en option.
projectmanagement-service = garde votre tableau identique sur vos ordinateurs via le serveur de Sicompass Cloud

projectmanagement-cloud-needs-payment = synchronisation cloud : demande Sicompass Cloud, voir store, abonnements
projectmanagement-cloud-active = synchronisation cloud : active, renouvellement dans { $days } jours
projectmanagement-cloud-grace = synchronisation cloud : abonnement expiré, encore active { $days } jours, renouvelez dans store, abonnements
projectmanagement-cloud-expired = synchronisation cloud : arrêtée, l'abonnement a expiré il y a { $days } jours, voir store, abonnements
projectmanagement-cloud-needs-subscription = la synchronisation cloud demande Sicompass Cloud, voir store, abonnements
projectmanagement-cloud-failed = échec de la synchronisation cloud : { $reason }
projectmanagement-error-cloud-row-undeletable = la ligne de la synchronisation cloud n'est pas une colonne, désactivez la synchronisation cloud dans les paramètres pour la retirer

projectmanagement-cmd-sync-now = synchroniser avec le cloud maintenant
projectmanagement-sync-pulled = les modifications de vos autres ordinateurs ont été ajoutées
projectmanagement-sync-conflicts = les modifications de vos autres ordinateurs ont été ajoutées, { $count } modifiées des deux côtés, la plus récente a été gardée
projectmanagement-sync-status-synced = cloud : synchronisé
projectmanagement-sync-status-changed = cloud : modifié depuis la dernière synchronisation
projectmanagement-sync-status-new = cloud : pas encore synchronisé

projectmanagement-tutorial = Gestion de projet, depuis le store : un tableau kanban. Appuyez sur ctrl+a pour ajouter une colonne, Droite pour l'ouvrir, et ctrl+a à l'intérieur pour ajouter une carte. Une colonne contient des cartes et rien de plus profond, donc une carte est le dernier niveau. La première ligne de chaque liste est son info de liste, avec le hash de la liste et, avec la synchronisation cloud active, si elle a changé depuis la dernière synchronisation.
projectmanagement-tutorial-2 = d : ouvrez le tableau lui-même, avec les colonnes côte à côte. Le curseur se pose sur une carte, et seule cette carte est mise en évidence. Gauche et Droite passent d'une colonne à l'autre, Haut et Bas parcourent les cartes d'une colonne. i et a modifient la carte, o et maj+o en ouvrent une nouvelle en dessous ou au dessus, ctrl+d supprime, et ctrl+x, ctrl+c et ctrl+v coupent, copient et collent. Une colonne vide montre un emplacement où vous poser pour ajouter sa première carte. Les colonnes elles-mêmes se gèrent dans la liste. Échap revient en arrière.
