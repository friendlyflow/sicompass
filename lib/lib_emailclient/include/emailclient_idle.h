#pragma once

#include "emailclient.h"

typedef void (*EmailIdleNotifyFn)(void *userdata);

/**
 * Start IMAP IDLE monitoring on a folder.
 * Spawns a background thread that maintains an IMAP IDLE connection.
 * When new mail arrives (EXISTS) or is removed (EXPUNGE), calls notifyFn
 * from the background thread.
 *
 * Only one IDLE session can be active at a time. Call emailclientIdleStop()
 * before starting a new one.
 */
bool emailclientIdleStart(const EmailClientConfig *config, const char *folder,
                          EmailIdleNotifyFn notifyFn, void *userdata);

/**
 * Stop the current IMAP IDLE session and join the background thread.
 * No-op if no session is active.
 */
void emailclientIdleStop(void);
