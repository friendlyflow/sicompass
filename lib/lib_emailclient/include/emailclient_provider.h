#pragma once

#include <provider_interface.h>

/**
 * Get the email client provider instance.
 *
 * The provider handles:
 * - Folder listing at root path
 * - Message header listing when navigated into a folder
 * - Message body display when navigated into a message
 * - Message sending via compose command
 *
 * @return Singleton Provider instance for emailclient
 */
Provider* emailclientGetProvider(void);
