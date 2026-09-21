# Strings shared by every provider that offers cloud backup.
#
# Kept plain: no em dash and no semicolon, because a screen reader reads these
# out and the punctuation gets in the way.

# The row a cloud-backed provider puts at the top of its first layer. It is
# always a link to the cloud tier, so only the wording changes.
payments-cloud-needs-payment = cloud backup: payment needed, enable cloud and store
payments-cloud-active = cloud backup: active, renews in { $days } days
payments-cloud-expired = cloud backup: subscription expired { $days } days ago
payments-cloud-invalid = cloud backup: the license certificate was rejected ({ $why })

# Spoken the moment the setting is switched on without a subscription.
payments-cloud-needs-subscription = cloud backup needs the cloud and store subscription
# Shown in the header when an upload fails.
payments-backup-failed = cloud backup failed: { $reason }

# The colon command that pulls the server's copy back over an empty store.
payments-command-restore = restore cloud backup
payments-restore-done = cloud backup restored
payments-restore-empty = there is no cloud backup to restore
payments-restore-refused = the local store is not empty, so nothing was restored
