#pragma once

#include <provider_interface.h>

/**
 * Get the filebrowser provider instance.
 *
 * The provider handles:
 * - Directory listing via <input>...</input> tagged elements
 * - File/directory renaming in insert mode
 * - Navigation through the filesystem
 *
 * @return Singleton Provider instance for filebrowser
 */
Provider* filebrowserGetProvider(void);
