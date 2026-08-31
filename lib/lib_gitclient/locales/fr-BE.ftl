# Git-client provider strings — Belgian French.

gitclient-display-name = client git

gitclient-head-branch = { $repo } sur { $branch }
gitclient-head-detached = { $repo }, detache sur { $oid }
gitclient-head-unborn = { $repo }, aucun commit
gitclient-head-unreadable = { $repo }, lecture impossible

gitclient-ahead = { $n } en avance
gitclient-behind = { $n } en retard
gitclient-in-sync = a jour
gitclient-no-upstream = pas de branche amont

# Noms de sections. Ce sont aussi les libelles par lesquels le curseur est
# retrouve apres une actualisation, donc sans compteur ni rien de variable.
gitclient-section-changes = modifications
gitclient-section-graph = graphe
gitclient-section-branches = branches
gitclient-section-stashes = remises
gitclient-section-remotes = depots distants

# L'espace finale est voulue : le message tape la suit sur la meme ligne.
gitclient-message-label = message :{" "}

gitclient-button-commit = valider
gitclient-button-amend = valider en corrigeant le dernier
gitclient-button-push = valider et pousser
gitclient-button-sync = valider et synchroniser

gitclient-clean = copie de travail propre

gitclient-change-modified = modifie
gitclient-change-added = ajoute
gitclient-change-deleted = supprime
gitclient-change-renamed = renomme
gitclient-change-copied = copie
gitclient-change-typechanged = type modifie
gitclient-change-untracked = non suivi
gitclient-change-changed = change
gitclient-renamed-from = depuis { $from }

gitclient-conflict = conflit
gitclient-conflict-both-modified = conflit, modifie des deux cotes
gitclient-conflict-both-added = conflit, ajoute des deux cotes
gitclient-conflict-both-deleted = conflit, supprime des deux cotes
gitclient-conflict-added-by-us = conflit, ajoute par nous
gitclient-conflict-added-by-them = conflit, ajoute par eux
gitclient-conflict-deleted-by-us = conflit, supprime par nous
gitclient-conflict-deleted-by-them = conflit, supprime par eux

gitclient-binary = fichier binaire, aucun texte a afficher
gitclient-unreadable = fichier illisible
gitclient-diff-truncated = { $n } lignes de plus, non affichees
gitclient-empty = rien ici
gitclient-gone = ceci n'existe plus, appuyez sur F5

gitclient-error-not-a-repository = pas un depot git

gitclient-no-commits = aucun commit
gitclient-load-more = charger { $n } commits de plus
gitclient-stats = { $files } fichiers modifies, { $ins } lignes ajoutees, { $del } lignes supprimees
gitclient-commit-author = auteur : { $author }
gitclient-commit-date = date : { $date }
gitclient-commit-refs = references : { $refs }

gitclient-scope-local = local
gitclient-scope-remote = distant
gitclient-no-branches = aucune branche
gitclient-branch-current = la branche ou vous etes
gitclient-branch-tracking = suit { $upstream }
gitclient-branch-gone = sa branche amont a disparu
gitclient-branch-elsewhere = extraite dans { $path }

gitclient-no-stashes = aucune remise
gitclient-no-remotes = aucun depot distant
gitclient-remote-fetch = recuperation : { $url }
gitclient-remote-push = envoi : { $url }

gitclient-working = { $what } en cours
gitclient-done = { $what } termine
gitclient-stale = le depot a change sur le disque, appuyez sur F5 pour recharger

gitclient-confirm-cancel = annuler, ne rien changer
gitclient-confirm-yes = oui, et perdre les modifications de { $what }

gitclient-error-locked = un autre processus git tourne dans ce depot, reessayez quand il aura fini
gitclient-error-select-a-file = placez d'abord le curseur sur un fichier
gitclient-error-select-a-commit = placez d'abord le curseur sur un commit
gitclient-error-select-a-branch = placez d'abord le curseur sur une branche
gitclient-error-select-a-stash = placez d'abord le curseur sur une remise
gitclient-error-select-a-remote = placez d'abord le curseur sur un depot distant
gitclient-error-not-staged = ce fichier n'est pas prepare
gitclient-error-remote-syntax = tapez un nom et une url, separes par un espace
gitclient-error-no-message = tapez d'abord un message, appuyez sur i sur la ligne du message
gitclient-error-conflicts = resolvez d'abord les conflits, preparez chaque fichier une fois corrige
gitclient-error-nothing-to-amend = il n'y a pas encore de commit a corriger
gitclient-error-busy = une autre operation distante est encore en cours
gitclient-error-undo = cela n'a pas pu etre annule
