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
 * An installed application with display name and executable command.
 */
typedef struct {
    char *name;  // Display name (e.g., "Firefox Web Browser")
    char *exec;  // Command to execute (e.g., "firefox")
} PlatformApplication;

/**
 * Get a list of installed applications.
 * - Linux: parses .desktop files from XDG application directories
 * - macOS: scans /Applications and ~/Applications for .app bundles
 * - Windows: enumerates App Paths registry key
 *
 * @param outCount Output parameter for the number of applications found
 * @return Array of PlatformApplication (caller must free via platformFreeApplications),
 *         or NULL on failure
 */
PlatformApplication* platformGetApplications(int *outCount);

/**
 * Free an array returned by platformGetApplications.
 */
void platformFreeApplications(PlatformApplication *apps, int count);

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
