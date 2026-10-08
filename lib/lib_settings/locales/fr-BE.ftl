# Settings provider strings — Belgian French.

settings-radio-color-scheme = jeu de couleurs
settings-radio-language = langue

# Color-scheme options
settings-colorScheme-option-dark = foncé
settings-colorScheme-option-light = clair

# Language options — shown in their native form (same across all locale files).
settings-language-option-en-US = English
settings-language-option-nl-BE = Nederlands (België)
settings-language-option-fr-BE = Français (Belgique)
settings-language-option-de-BE = Deutsch (Belgien)

# Section display names. "sicompass" is the product name and stays in all
# locales.
settings-section-sicompass = sicompass
settings-section-file-browser = navigateur de fichiers
settings-section-web-browser = navigateur web
settings-section-email-client = client de messagerie
settings-section-chat-client = client de chat
settings-section-terminal = terminal
settings-section-text-editor = éditeur de texte
settings-section-tutorial = tutoriel

# Setting labels
settings-label-version = version
settings-label-version-app = version (application)
settings-label-version-sdk = version (SDK)
settings-checkbox-maximized = agrandi
settings-checkbox-shoulder-surfing-protection = protection contre l'espionnage (écran vide)
settings-checkbox-screen-reader = lecteur d'écran
settings-screen-reader-failed = Impossible de démarrer le lecteur d'écran : { $error }
settings-checkbox-auto-update-check = vérifier les mises à jour au démarrage
settings-onboarding-body = Utilisez Haut et Bas pour parcourir la liste. Appuyez deux fois sur Home pour atteindre la racine et voir tous vos programmes disponibles. Droite ouvre un élément qui affiche un signe plus (+), Gauche remonte au parent, Entrée coche une case, Échap revient en arrière.
settings-radio-font-scale = taille du texte
settings-radio-sort-order = ordre de tri

# Sort-order options
settings-sortOrder-option-alphanumerically = alphanumérique
settings-sortOrder-option-chronologically = chronologique

# Messages d'erreur
settings-error-malformed-undo-payload = paramètres : charge utile d'annulation invalide
settings-error-malformed-redo-payload = paramètres : charge utile de répétition invalide

# Mises à jour prêtes à installer (le message en haut et Ctrl+U)
updates-ready = Mises à jour prêtes : { $list } (Ctrl+U pour installer)
updates-app = sicompass { $version }
updates-programs = { $count ->
    [one] 1 programme
   *[other] { $count } programmes
}
updates-need-approval = { $count ->
    [one] 1 mise à jour de programme demande plus d'accès : approuvez-la dans le store
   *[other] { $count } mises à jour de programmes demandent plus d'accès : approuvez-les dans le store
}
updates-installing = { $count ->
    [one] Installation de 1 mise à jour de programme…
   *[other] Installation de { $count } mises à jour de programmes…
}
updates-installed = { $name } est mis à jour vers { $version }.
updates-failed = Impossible de mettre à jour { $name } : { $error }
updates-none = Aucune mise à jour à installer.
updates-release-page-opened = La page de la version est ouverte dans le navigateur.
updates-apply-failed = Impossible d'installer la mise à jour : { $error }
