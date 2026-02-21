#pragma once

#include <stdbool.h>
#include <ffon.h>

typedef enum {
    FILEBROWSER_SORT_ALPHA = 0,
    FILEBROWSER_SORT_CHRONO,
} FilebrowserSortMode;

/**
 * List the contents of a directory at the given URI.
 *
 * @param uri The directory path (must start with '/')
 * @param commands If true, include executable files; if false, exclude them
 * @param showProperties If true, prepend ls-al-style stat info before the <input> tag
 * @param sortMode FILEBROWSER_SORT_ALPHA for case-insensitive alphabetical order,
 *                 FILEBROWSER_SORT_CHRONO for newest-first by modification time
 * @param out_count Output parameter for the number of elements in the returned array
 * @return Array of FfonElement* containing directories and files, or NULL on error.
 *         Directories are returned as FFON_OBJECT elements.
 *         Files are returned as FFON_STRING elements.
 *         Names are wrapped in <input>...</input> tags for inline editing.
 *         When showProperties is true, stat info is prepended before the <input> tag.
 *         Caller is responsible for freeing the returned array and its elements.
 */
FfonElement** filebrowserListDirectory(const char *uri, bool commands, bool showProperties,
                                       FilebrowserSortMode sortMode, int *out_count);

/**
 * Rename a file or directory.
 *
 * @param uri The parent directory path (must start with '/')
 * @param oldName The current name
 * @param newName The new name
 * @return true on success, false on failure
 */
bool filebrowserRename(const char *uri, const char *oldName, const char *newName);

/**
 * Create a new directory.
 *
 * @param uri The parent directory path (must start with '/')
 * @param name The directory name to create
 * @return true on success, false on failure
 */
bool filebrowserCreateDirectory(const char *uri, const char *name);

/**
 * Create a new empty file.
 *
 * @param uri The parent directory path (must start with '/')
 * @param name The file name to create
 * @return true on success, false on failure
 */
bool filebrowserCreateFile(const char *uri, const char *name);

/**
 * Delete a file or directory (recursively) at uri/name.
 *
 * @param uri The parent directory path (must start with '/')
 * @param name The file or directory name (trailing slash allowed for dirs)
 * @return true on success, false on failure
 */
bool filebrowserDelete(const char *uri, const char *name);

