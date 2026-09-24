# Textes partages par chaque fournisseur proposant la sauvegarde cloud.
#
# Volontairement simple: pas de tiret cadratin ni de point-virgule, car un
# lecteur d'ecran lit ces lignes a voix haute.

payments-cloud-needs-payment = sauvegarde cloud : paiement requis, prenez Sicompass Cloud
payments-cloud-active = sauvegarde cloud: active, renouvellement dans { $days } jours
payments-cloud-expired = sauvegarde cloud: abonnement expire il y a { $days } jours
payments-cloud-grace = sauvegarde cloud : abonnement expiré, encore active { $days } jours, renouvelez pour la garder
payments-cloud-invalid = sauvegarde cloud: le certificat de licence a ete refuse ({ $why })

payments-cloud-needs-subscription = la sauvegarde cloud demande Sicompass Cloud, anciennement cloud et magasin
payments-backup-failed = echec de la sauvegarde cloud: { $reason }

payments-command-restore = restaurer la sauvegarde cloud
payments-restore-done = sauvegarde cloud restauree
payments-restore-empty = il n'y a aucune sauvegarde cloud a restaurer
payments-restore-refused = le stockage local n'est pas vide, rien n'a ete restaure

# What the cloud backup uses, as the server last reported it (shown in the Store).
payments-usage-stored = { $used } sur { $cap } conservés dans le cloud
payments-usage-transferred = { $used } sur { $cap } transférés ce mois-ci
