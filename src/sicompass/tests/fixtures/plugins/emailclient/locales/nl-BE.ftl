# Email-client provider strings — Belgian Dutch (Flemish).

emailclient-display-name = e-mailclient
emailclient-description = E-mail als lijst: je mappen, hun berichten en een formulier om er een te schrijven. Meld je aan met Google.
emailclient-setting-imap-url = IMAP-URL
emailclient-setting-smtp-url = SMTP-URL
emailclient-setting-username = gebruikersnaam
emailclient-setting-password = wachtwoord
emailclient-setting-client-id = client-ID (OAuth)
emailclient-setting-client-secret = clientgeheim (OAuth)

# Foutmeldingen
emailclient-error-not-connected = niet verbonden
emailclient-error-imap-move-failed = { $label } imap-move mislukt: { $err }
emailclient-error-imap-move-no-longer-in = { $label } imap-move: bericht niet meer in { $searchin }
emailclient-error-set-seen-failed = { $label } gelezen-markeren mislukt: { $err }
emailclient-error-set-flagged-failed = { $label } ster-zetten mislukt: { $err }
emailclient-error-command-failed = { $cmd } mislukt: { $err }
emailclient-error-cmd-not-viewing = { $cmd }: geen bericht geopend
emailclient-error-delete-failed = verwijderen mislukt: { $err }
emailclient-error-delete-not-viewing = verwijderen: geen bericht geopend
emailclient-error-archive-no-folder = archiveren: server biedt geen \Archive-map aan
emailclient-error-archive-failed = archiveren mislukt: { $err }
emailclient-error-archive-not-viewing = archiveren: geen bericht geopend
emailclient-error-move-not-viewing = verplaatsen: geen bericht geopend
emailclient-error-unknown-command = onbekend commando: { $cmd }

emailclient-tutorial = E-mail, uit de store: IMAP en SMTP met Gmail OAuth. Mappen en berichten vormen een boom, en je kunt opstellen, beantwoorden, verplaatsen en verwijderen, allemaal undoable.
emailclient-tutorial-2 = Gmail instellen: Gmail gebruikt OAuth, geen wachtwoord. Maak in de Google Cloud Console een project aan, schakel de Gmail API in, en voeg op het OAuth-toestemmingsscherm de scope https://mail.google.com/ toe. Die mail-scope is degene die telt. Zonder die scope geeft Google een token zonder mailtoegang en mislukt de aanmelding met invalid credentials. Maak daarna een OAuth-client-ID aan, plak de client-ID en het secret in Instellingen, en meld je aan. Let erop dat het Google-toestemmingsscherm echt vraagt om je mail te lezen en te verzenden. Als je vastloopt, voer je het dubbelpuntcommando :refresh uit om opnieuw op te halen, en om opnieuw te autoriseren voer je :logout uit en meld je je weer aan zodat een nieuw token met de mail-scope wordt aangemaakt.
