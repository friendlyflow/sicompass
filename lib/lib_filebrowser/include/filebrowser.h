#pragma once

#include <stdbool.h>
#include <ffon.h>

/**
 * List the contents of a directory at the given URI.
 *
 * @param uri The directory path (must start with '/')
 * @param commands If true, include executable files; if false, exclude them
 * @param out_count Output parameter for the number of elements in the returned array
 * @return Array of FfonElement* containing directories and files, or NULL on error.
 *         Directories are returned as FFON_OBJECT elements.
 *         Files are returned as FFON_STRING elements.
 *         Names are wrapped in <input>...</input> tags for inline editing.
 *         Caller is responsible for freeing the returned array and its elements.
 */
FfonElement** filebrowserListDirectory(const char *uri, bool commands, int *out_count);

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
 * Check if text contains <input>...</input> tags.
 *
 * @param text The text to check
 * @return true if text contains input tags
 */
bool filebrowserHasInputTags(const char *text);

/**
 * Extract content between <input> and </input> tags.
 * Caller must free the returned string.
 *
 * @param text The text containing input tags
 * @return The content between tags, or NULL if no tags found
 */
char* filebrowserExtractInputContent(const char *text);

/**
 * Strip <input> and </input> tags from text.
 * Caller must free the returned string.
 *
 * @param text The text containing input tags
 * @return The text with tags removed
 */
char* filebrowserStripInputTags(const char *text);
