# Email-client provider strings — Belgian French.

emailclient-display-name = client de messagerie
emailclient-description = Le courrier électronique sous forme de liste : vos dossiers, leurs messages et un formulaire pour en écrire un. Connectez-vous avec Google.
emailclient-setting-imap-url = URL IMAP
emailclient-setting-smtp-url = URL SMTP
emailclient-setting-username = nom d'utilisateur
emailclient-setting-password = mot de passe
emailclient-setting-client-id = ID client (OAuth)
emailclient-setting-client-secret = secret client (OAuth)

# Messages d'erreur
emailclient-error-not-connected = pas connecté
emailclient-error-imap-move-failed = { $label } imap-move échoué : { $err }
emailclient-error-imap-move-no-longer-in = { $label } imap-move : message plus dans { $searchin }
emailclient-error-set-seen-failed = { $label } marquer-lu échoué : { $err }
emailclient-error-set-flagged-failed = { $label } étoile-marquer échoué : { $err }
emailclient-error-command-failed = { $cmd } échoué : { $err }
emailclient-error-cmd-not-viewing = { $cmd } : aucun message ouvert
emailclient-error-delete-failed = suppression échouée : { $err }
emailclient-error-delete-not-viewing = supprimer : aucun message ouvert
emailclient-error-archive-no-folder = archiver : le serveur n'annonce pas de dossier \Archive
emailclient-error-archive-failed = archivage échoué : { $err }
emailclient-error-archive-not-viewing = archiver : aucun message ouvert
emailclient-error-move-not-viewing = déplacer : aucun message ouvert
emailclient-error-unknown-command = commande inconnue : { $cmd }

emailclient-tutorial = Messagerie, depuis le store : IMAP et SMTP avec Gmail OAuth. Dossiers et messages forment un arbre, et vous pouvez composer, répondre, déplacer et supprimer, le tout annulable.
emailclient-tutorial-2 = Configurer Gmail : Gmail utilise OAuth, pas un mot de passe. Dans la Google Cloud Console, créez un projet, activez l'API Gmail, et sur l'écran de consentement OAuth ajoutez la portée https://mail.google.com/. Cette portée mail est celle qui compte. Sans elle, Google délivre un jeton sans accès au courrier et la connexion échoue avec invalid credentials. Créez ensuite un identifiant client OAuth, collez son identifiant client et son secret dans Paramètres, et connectez-vous. Vérifiez que l'écran de consentement Google demande bien de lire et d'envoyer votre courrier. Si vous êtes bloqué, lancez la commande deux-points :refresh pour forcer une nouvelle récupération, et pour réautoriser lancez :logout puis reconnectez-vous afin qu'un nouveau jeton soit émis avec la portée mail.
