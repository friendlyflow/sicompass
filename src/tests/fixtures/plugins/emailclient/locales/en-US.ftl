# Email-client provider strings — English (source/fallback).

emailclient-display-name = email client
emailclient-description = Email as a list: your folders, their messages, and a form to write one. Sign in with Google.
emailclient-setting-imap-url = IMAP URL
emailclient-setting-smtp-url = SMTP URL
emailclient-setting-username = username
emailclient-setting-password = password
emailclient-setting-client-id = client ID (OAuth)
emailclient-setting-client-secret = client secret (OAuth)

# Error messages
emailclient-error-not-connected = not connected
emailclient-error-imap-move-failed = { $label } imap-move failed: { $err }
emailclient-error-imap-move-no-longer-in = { $label } imap-move: message no longer in { $searchin }
emailclient-error-set-seen-failed = { $label } set-seen failed: { $err }
emailclient-error-set-flagged-failed = { $label } set-flagged failed: { $err }
emailclient-error-command-failed = { $cmd } failed: { $err }
emailclient-error-cmd-not-viewing = { $cmd }: not viewing a message
emailclient-error-delete-failed = delete failed: { $err }
emailclient-error-delete-not-viewing = delete: not viewing a message
emailclient-error-archive-no-folder = archive: server does not advertise an \Archive folder
emailclient-error-archive-failed = archive failed: { $err }
emailclient-error-archive-not-viewing = archive: not viewing a message
emailclient-error-move-not-viewing = move: not viewing a message
emailclient-error-unknown-command = unknown command: { $cmd }

# The tutorial's paragraphs about this program, under its programs section:
# <name>-tutorial, then <name>-tutorial-2 and so on, read until one is missing.
emailclient-tutorial = Email, from the Store: IMAP and SMTP with Gmail OAuth. Folders and messages form a tree, and you can compose, reply, move, and delete, all undoable.
emailclient-tutorial-2 = Set up Gmail: Gmail uses OAuth, not a password. In the Google Cloud Console, create a project, enable the Gmail API, and on the OAuth consent screen add the scope https://mail.google.com/. That mail scope is the one that matters. Without it, Google issues a token with no mail access and login fails with invalid credentials. Then create an OAuth client ID, paste its client ID and secret into Settings, and log in. Watch that the Google consent screen actually asks to read and send your mail. If you get stuck, run the colon command :refresh to force a re-fetch, and to re-authorize run :logout and then log in again so a fresh token is minted with the mail scope.
