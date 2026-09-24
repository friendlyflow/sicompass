# Strings shared by every provider that offers cloud backup.
#
# Kept plain: no em dash and no semicolon, because a screen reader reads these
# out and the punctuation gets in the way.

# The row a cloud-backed provider puts at the top of its first layer. It is
# always a link to the cloud tier, so only the wording changes.
payments-cloud-needs-payment = cloud backup: payment needed, get Sicompass Cloud
payments-cloud-active = cloud backup: active, renews in { $days } days
payments-cloud-expired = cloud backup: subscription expired { $days } days ago
payments-cloud-grace = cloud backup: subscription expired, still on for { $days } days, renew to keep it on
payments-cloud-invalid = cloud backup: the license certificate was rejected ({ $why })

# Spoken the moment the setting is switched on without a subscription.
payments-cloud-needs-subscription = cloud backup needs Sicompass Cloud, formerly called cloud and store
# Shown in the header when an upload fails.
payments-backup-failed = cloud backup failed: { $reason }

# The colon command that pulls the server's copy back over an empty store.
payments-command-restore = restore cloud backup
payments-restore-done = cloud backup restored
payments-restore-empty = there is no cloud backup to restore
payments-restore-refused = the local store is not empty, so nothing was restored

# What the cloud backup uses, as the server last reported it (shown in the Store).
payments-usage-stored = { $used } of { $cap } kept in the cloud
payments-usage-transferred = { $used } of { $cap } moved this month
