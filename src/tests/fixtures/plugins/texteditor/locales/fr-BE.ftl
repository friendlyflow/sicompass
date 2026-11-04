# Text-editor provider strings — Belgian French.

texteditor-display-name = éditeur de texte

# Messages d'erreur
texteditor-error-undo-delete-line-write-failed = annuler la suppression de ligne : écriture du fichier échouée
texteditor-error-undo-insert-line-write-failed = annuler l'insertion de ligne : écriture du fichier échouée
texteditor-error-redo-delete-line-write-failed = refaire la suppression de ligne : écriture du fichier échouée
texteditor-error-redo-insert-line-write-failed = refaire l'insertion de ligne : écriture du fichier échouée
texteditor-error-redo-delete-trash-failed = refaire la suppression : corbeille échouée : { $err }
texteditor-error-create-file = impossible de créer { $name } : { $err }
texteditor-error-create-directory = impossible de créer le dossier { $name } : { $err }
texteditor-error-delete = impossible de supprimer { $name } : { $err }
texteditor-error-rename = impossible de renommer { $old } en { $new } : { $err }
texteditor-error-save = impossible d'enregistrer { $name } : { $err }
texteditor-error-add-here = impossible d'ajouter dans { $path } : { $err }
texteditor-error-change-here = impossible de modifier { $path } : { $err }
texteditor-error-reason-permission-denied = permission refusée

# A .ff file: a tree of nodes, each hashed over its children and its text.
# Every level of a .ff file opens with this row. Inside it: the hash of the
# node whose children the level shows.
texteditor-ff-header = en-tête
texteditor-ff-sha256 = sha256 : { $hash }
texteditor-ff-invalid = (fichier .ff non valide : { $err })
texteditor-ff-reason-truncated = il s'arrête au milieu d'un nœud
texteditor-ff-reason-trailing = il reste des octets après son nœud racine
texteditor-ff-reason-utf8 = le texte d'un nœud n'est pas en UTF-8
texteditor-ff-reason-too-deep = ses nœuds sont imbriqués sur plus de 256 niveaux
texteditor-ff-reason-no-node = ce nœud n'existe plus
texteditor-error-ff-hash = { $name } a été modifié hors de sicompass ou est endommagé, ses hachages ne correspondent pas. L'enregistrer en écrit de nouveaux.
texteditor-error-ff-too-long = un nœud contient au plus 255 octets de texte
texteditor-error-ff-too-many = un nœud a au plus 255 enfants
texteditor-error-undo-ff-write-failed = annuler : impossible d'écrire le fichier .ff
texteditor-error-redo-ff-write-failed = rétablir : impossible d'écrire le fichier .ff

texteditor-description = Modifiez tout fichier texte comme une liste de ses lignes, avec l'indentation et les sections comme niveaux, et chaque modification annulable.
texteditor-setting-path = chemin de l'éditeur de texte

texteditor-tutorial = Éditeur de texte, depuis le store : appuyez sur Droite sur un fichier pour ouvrir son contenu sous forme d'arbre et modifier les lignes sur place. Chaque changement est sur la chronologie d'annulation.
