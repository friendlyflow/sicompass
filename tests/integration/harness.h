#pragma once

#include "app_state.h"
#include <provider_interface.h>

// Create an AppRenderer without SDL/Vulkan/AccessKit init.
// Sets up all buffers, IDs, undo history, and caret stub.
AppRenderer* harnessCreateAppRenderer(void);

// Destroy an AppRenderer created by harnessCreateAppRenderer.
void harnessDestroyAppRenderer(AppRenderer *appRenderer);

// Set up file browser + settings providers and populate appRenderer->ffon/providers.
// fbTmpDir: temp directory for file browser to operate in.
// Returns true on success.
bool harnessSetupProviders(AppRenderer *appRenderer, const char *fbTmpDir);

// Key simulation
void pressKey(AppRenderer *app, SDL_Keycode key, SDL_Keymod mod);
void pressDown(AppRenderer *app);
void pressUp(AppRenderer *app);
void pressRight(AppRenderer *app);
void pressLeft(AppRenderer *app);
void pressEnter(AppRenderer *app);
void pressEscape(AppRenderer *app);
void pressTab(AppRenderer *app);
void pressCtrl(AppRenderer *app, SDL_Keycode key);
void pressCtrlShift(AppRenderer *app, SDL_Keycode key);
void typeText(AppRenderer *app, const char *text);
