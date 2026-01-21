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
 *         Directories are returned as FFON_OBJECT elements with key ending in '/'.
 *         Files are returned as FFON_STRING elements.
 *         Caller is responsible for freeing the returned array and its elements.
 */
FfonElement** filebrowserListDirectory(const char *uri, bool commands, int *out_count);
