#pragma once

#include <provider_interface.h>

/**
 * Get the chat client provider instance.
 *
 * The provider handles:
 * - Room listing at root path
 * - Message display when navigated into a room
 * - Message sending via inline input and command system
 *
 * @return Singleton Provider instance for chatclient
 */
Provider* chatclientGetProvider(void);
