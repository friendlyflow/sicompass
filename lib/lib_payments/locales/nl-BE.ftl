# Gedeelde teksten voor elke provider met cloudback-up.
#
# Bewust eenvoudig: geen kastlijntje en geen kommapunt, want een schermlezer
# leest deze regels voor en die leestekens zitten in de weg.

payments-cloud-needs-payment = cloudback-up: betaling nodig, neem Sicompass Cloud
payments-cloud-active = cloudback-up: actief, verlengt over { $days } dagen
payments-cloud-expired = cloudback-up: abonnement { $days } dagen geleden verlopen
payments-cloud-grace = cloudback-up: abonnement verlopen, nog { $days } dagen aan, vernieuw om het aan te houden
payments-cloud-invalid = cloudback-up: het licentiecertificaat werd geweigerd ({ $why })

payments-cloud-needs-subscription = cloudback-up heeft Sicompass Cloud nodig, vroeger cloud en winkel
payments-backup-failed = cloudback-up mislukt: { $reason }

payments-command-restore = cloudback-up terugzetten
payments-restore-done = cloudback-up teruggezet
payments-restore-empty = er is geen cloudback-up om terug te zetten
payments-restore-refused = de lokale opslag is niet leeg, er werd niets teruggezet

# What the cloud backup uses, as the server last reported it (shown in the Store).
payments-usage-stored = { $used } van { $cap } bewaard in de cloud
payments-usage-transferred = { $used } van { $cap } verplaatst deze maand
