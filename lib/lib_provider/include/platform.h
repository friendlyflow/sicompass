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

/**
 * Get a list of executable program names found in the system PATH.
 *
 * @param outCount Output parameter for the number of programs found
 * @return Array of newly allocated strings (caller must free via platformFreePathExecutables),
 *         or NULL on failure
 */
char** platformGetPathExecutables(int *outCount);

/**
 * Free an array of strings returned by platformGetPathExecutables.
 */
void platformFreePathExecutables(char **executables, int count);

/**
 * Open a file with a specific program.
 * - Linux: runs "program \"filePath\" &"
 * - macOS: runs "open -a \"program\" \"filePath\" &"
 * - Windows: uses ShellExecuteA with program as the executable
 *
 * @param program The program name or path
 * @param filePath The file path to open
 * @return true on success, false on failure
 */
bool platformOpenWith(const char *program, const char *filePath);
