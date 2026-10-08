# Settings provider strings — Belgian German (Eastern Cantons).

settings-radio-color-scheme = Farbschema
settings-radio-language = Sprache

# Color-scheme options
settings-colorScheme-option-dark = dunkel
settings-colorScheme-option-light = hell

# Language options — shown in their native form (same across all locale files).
settings-language-option-en-US = English
settings-language-option-nl-BE = Nederlands (België)
settings-language-option-fr-BE = Français (Belgique)
settings-language-option-de-BE = Deutsch (Belgien)

# Section display names. "sicompass" is the product name and stays in all
# locales.
settings-section-sicompass = sicompass
settings-section-file-browser = Dateimanager
settings-section-web-browser = Webbrowser
settings-section-email-client = E-Mail-Client
settings-section-chat-client = Chat-Client
settings-section-terminal = Terminal
settings-section-text-editor = Texteditor
settings-section-tutorial = Anleitung

# Setting labels
settings-label-version = Version
settings-label-version-app = Version (App)
settings-label-version-sdk = Version (SDK)
settings-checkbox-maximized = maximiert
settings-checkbox-shoulder-surfing-protection = Sichtschutz (leerer Bildschirm)
settings-checkbox-screen-reader = Bildschirmleser
settings-screen-reader-failed = Der Bildschirmleser konnte nicht gestartet werden: { $error }
settings-checkbox-auto-update-check = beim Start nach Updates suchen
settings-onboarding-body = Mit Hoch und Runter bewegst du dich durch die Liste. Drücke zweimal Home, um zur Wurzel zu gelangen und alle deine verfügbaren Programme zu sehen. Rechts öffnet einen Eintrag mit einem Pluszeichen (+), Links geht zum übergeordneten Eintrag, Enter aktiviert ein Kontrollkästchen, Escape geht zurück.
settings-radio-font-scale = Schriftgröße
settings-radio-sort-order = Sortierreihenfolge

# Sort-order options
settings-sortOrder-option-alphanumerically = alphanumerisch
settings-sortOrder-option-chronologically = chronologisch

# Fehlermeldungen
settings-error-malformed-undo-payload = Einstellungen: ungültige Undo-Nutzlast
settings-error-malformed-redo-payload = Einstellungen: ungültige Redo-Nutzlast

# Updates bereit zur Installation (die Meldung oben und Ctrl+U)
updates-ready = Updates bereit: { $list } (Ctrl+U zum Installieren)
updates-app = sicompass { $version }
updates-programs = { $count ->
    [one] 1 Programm
   *[other] { $count } Programme
}
updates-need-approval = { $count ->
    [one] 1 Programm-Update verlangt mehr Zugriff: im Store genehmigen
   *[other] { $count } Programm-Updates verlangen mehr Zugriff: im Store genehmigen
}
updates-installing = { $count ->
    [one] 1 Programm-Update wird installiert…
   *[other] { $count } Programm-Updates werden installiert…
}
updates-installed = { $name } ist auf { $version } aktualisiert.
updates-failed = { $name } konnte nicht aktualisiert werden: { $error }
updates-none = Keine Updates zu installieren.
updates-release-page-opened = Die Release-Seite wurde im Browser geöffnet.
updates-apply-failed = Das Update konnte nicht installiert werden: { $error }
