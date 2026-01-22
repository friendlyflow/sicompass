#pragma once

#include <stdbool.h>

/**
 * Cross-platform utilities for file operations and paths.
 */

/**
 * Open a file or URL with the system's default application.
 * - Linux: uses xdg-open
 * - macOS: uses open
 * - Windows: uses ShellExecuteA
 *
 * @param path The file path or URL to open
 * @return true on success, false on failure
 */
bool platformOpenWithDefault(const char *path);

/**
 * Get the user's config directory for the application.
 * - Linux: $XDG_CONFIG_HOME or ~/.config/
 * - macOS: ~/Library/Application Support/
 * - Windows: %APPDATA%/
 *
 * @return Newly allocated path string (caller must free), or NULL on failure
 */
char* platformGetConfigHome(void);

/**
 * Get the user's home directory.
 * - Linux/macOS: $HOME
 * - Windows: %USERPROFILE%
 *
 * @return Newly allocated path string (caller must free), or NULL on failure
 */
char* platformGetHomeDir(void);

/**
 * Get the path separator for the current platform.
 * - Linux/macOS: "/"
 * - Windows: "\\"
 */
const char* platformGetPathSeparator(void);

/**
 * Check if running on Windows.
 */
bool platformIsWindows(void);
