# Email-client provider strings — Belgian German (Eastern Cantons).

emailclient-display-name = E-Mail-Client
emailclient-description = E-Mail als Liste: Ihre Ordner, deren Nachrichten und ein Formular, um eine zu schreiben. Melden Sie sich mit Google an.
emailclient-setting-imap-url = IMAP-URL
emailclient-setting-smtp-url = SMTP-URL
emailclient-setting-username = Benutzername
emailclient-setting-password = Passwort
emailclient-setting-client-id = Client-ID (OAuth)
emailclient-setting-client-secret = Client-Geheimnis (OAuth)

# Fehlermeldungen
emailclient-error-not-connected = nicht verbunden
emailclient-error-imap-move-failed = { $label } imap-move fehlgeschlagen: { $err }
emailclient-error-imap-move-no-longer-in = { $label } imap-move: Nachricht nicht mehr in { $searchin }
emailclient-error-set-seen-failed = { $label } Gelesen-Markieren fehlgeschlagen: { $err }
emailclient-error-set-flagged-failed = { $label } Sterne-Markieren fehlgeschlagen: { $err }
emailclient-error-command-failed = { $cmd } fehlgeschlagen: { $err }
emailclient-error-cmd-not-viewing = { $cmd }: keine Nachricht geöffnet
emailclient-error-delete-failed = Löschen fehlgeschlagen: { $err }
emailclient-error-delete-not-viewing = Löschen: keine Nachricht geöffnet
emailclient-error-archive-no-folder = Archivieren: Server bietet keinen \Archive-Ordner
emailclient-error-archive-failed = Archivieren fehlgeschlagen: { $err }
emailclient-error-archive-not-viewing = Archivieren: keine Nachricht geöffnet
emailclient-error-move-not-viewing = Verschieben: keine Nachricht geöffnet
emailclient-error-unknown-command = unbekannter Befehl: { $cmd }

emailclient-tutorial = E-Mail, aus dem Store: IMAP und SMTP mit Gmail OAuth. Ordner und Nachrichten bilden einen Baum, und Sie können verfassen, antworten, verschieben und löschen, alles rückgängig zu machen.
emailclient-tutorial-2 = Gmail einrichten: Gmail verwendet OAuth, kein Passwort. Erstellen Sie in der Google Cloud Console ein Projekt, aktivieren Sie die Gmail-API, und fügen Sie auf dem OAuth-Zustimmungsbildschirm den Scope https://mail.google.com/ hinzu. Dieser Mail-Scope ist der entscheidende. Ohne ihn stellt Google ein Token ohne Mailzugriff aus und die Anmeldung schlägt mit invalid credentials fehl. Erstellen Sie danach eine OAuth-Client-ID, fügen Sie deren Client-ID und Secret in die Einstellungen ein, und melden Sie sich an. Achten Sie darauf, dass der Google-Zustimmungsbildschirm tatsächlich um das Lesen und Senden Ihrer Mail bittet. Wenn Sie feststecken, führen Sie den Doppelpunktbefehl :refresh aus, um erneut abzurufen, und zum erneuten Autorisieren führen Sie :logout aus und melden sich wieder an, damit ein frisches Token mit dem Mail-Scope erstellt wird.
