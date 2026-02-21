#include "view.h"
#include "provider.h"
#include "programs.h"
#include <provider_interface.h>
#include <settings_provider.h>
#include <string.h>

// Palette definitions
const ColorPalette PALETTE_DARK = {
    .background         = 0x000000FF,
    .text               = 0xFFFFFFFF,
    .headerseparator    = 0x333333FF,
    .selected           = 0x2D4A28FF,
    .extsearch          = 0x696969FF,
    .scrollsearch       = 0x264F78FF,
    .error              = 0xFF0000FF,
};

const ColorPalette PALETTE_LIGHT = {
    .background         = 0xFFFFFFFF,
    .text               = 0x000000FF,
    .headerseparator    = 0xE0E0E0FF,
    .selected           = 0xC0ECB8FF,
    .extsearch          = 0x333333FF,
    .scrollsearch       = 0xA8C7FAFF,
    .error              = 0xFF0000FF,
};

static void applySettings(const char *key, const char *value, void *userdata) {
    AppRenderer *appRenderer = (AppRenderer *)userdata;
    if (!appRenderer) return;
    if (strcmp(key, "colorScheme") == 0) {
        appRenderer->palette = (strcmp(value, "light") == 0) ? &PALETTE_LIGHT : &PALETTE_DARK;
        appRenderer->needsRedraw = true;
        return;
    }
    if (strcmp(key, "sortOrder") == 0) {
        const char *cmd = strcmp(value, "chronologically") == 0
            ? "sort chronologically" : "sort alphanumerically";
        for (int i = 0; i < appRenderer->ffonCount; i++) {
            if (strcmp(appRenderer->providers[i]->name, "filebrowser") == 0) {
                Provider *fb = appRenderer->providers[i];
                if (fb->handleCommand) {
                    fb->handleCommand(fb, cmd, NULL, 0, NULL, 0);
                }
                break;
            }
        }
        appRenderer->needsRedraw = true;
    }
}

void mainLoop(SiCompassApplication* app) {
    // Initialize app renderer
    app = appRendererCreate(app);
    if (!app->appRenderer) {
        fprintf(stderr, "Failed to create editor state\n");
        return;
    }

    // Initialize palette before any rendering
    app->appRenderer->palette = &PALETTE_DARK;

    // Create settings before programs so they can register their settings sections
    Provider *settingsProvider = settingsProviderCreate(applySettings, app->appRenderer);

    // Load programs from config - registers providers and their settings
    programsLoad(settingsProvider);

    // Settings always registered last
    providerRegister(settingsProvider);

    providerInitAll();  // triggers settingsInit → loads config → applies palette

    // Build root array dynamically from all registered providers
    int total = providerGetRegisteredCount();
    app->appRenderer->ffon = malloc(total * sizeof(FfonElement*));
    app->appRenderer->providers = malloc(total * sizeof(Provider*));
    app->appRenderer->ffonCapacity = total;
    int providerCount = 0;
    for (int i = 0; i < total; i++) {
        Provider *p = providerGetRegisteredAt(i);
        FfonElement *elem = providerGetInitialElement(p);
        if (elem) {
            app->appRenderer->ffon[providerCount] = elem;
            app->appRenderer->providers[providerCount] = p;
            providerCount++;
        }
    }
    app->appRenderer->ffonCount = providerCount;
    if (providerCount == 0) {
        fprintf(stderr, "Failed to get initial elements from providers\n");
        appRendererDestroy(app->appRenderer);
        return;
    }

    // Initialize current_id - start at first provider object
    idArrayInit(&app->appRenderer->currentId);
    idArrayPush(&app->appRenderer->currentId, 0);

    // Set initial coordinate
    app->appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    app->appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;

    // Initialize list for initial coordinate
    createListCurrentLayer(app->appRenderer);

    // Initial render
    app->appRenderer->needsRedraw = true;
    updateView(app);

    // Main event loop
    SDL_Event event;
    while (app->running) {
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
                case SDL_EVENT_QUIT:
                    app->running = false;
                    break;

                case SDL_EVENT_KEY_DOWN:
                    if (event.key.windowID != app->windowId) break;
                    handleKeys(app->appRenderer, &event);
                    // Enable/disable text input based on current mode
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
                        SDL_StartTextInput(app->window);
                    } else {
                        SDL_StopTextInput(app->window);
                    }
                    break;

                case SDL_EVENT_TEXT_INPUT:
                    if (event.text.windowID != app->windowId) break;
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
                        handleInput(app->appRenderer, event.text.text);
                    }
                    break;

                case SDL_EVENT_WINDOW_RESIZED:
                case SDL_EVENT_WINDOW_MAXIMIZED:
                case SDL_EVENT_WINDOW_EXPOSED:
                    if (event.window.windowID != app->windowId) break;
                    app->framebufferResized = true;
                    app->appRenderer->needsRedraw = true;
                    break;

                case SDL_EVENT_WINDOW_FOCUS_GAINED:
                    if (event.window.windowID != app->windowId) break;
                    accesskitUpdateWindowFocus(app->appRenderer, true);
                    break;

                case SDL_EVENT_WINDOW_FOCUS_LOST:
                    if (event.window.windowID != app->windowId) break;
                    accesskitUpdateWindowFocus(app->appRenderer, false);
                    break;

                default:
                    // Handle custom accessibility events
                    if (event.type == app->userEvent) {
                        if (event.user.windowID != app->windowId) break;
                        accesskit_node_id target = (accesskit_node_id)((uintptr_t)(event.user.data1));
                        if (target == ELEMENT_ID) {
                            windowStateLock(&app->appRenderer->state);
                            if (event.user.code == SET_FOCUS_MSG) {
                                handleKeys(app->appRenderer, &event);
                            }
                            windowStateUnlock(&app->appRenderer->state);
                        }
                    }
                    break;
            }
        }

        // Update caret blink state
        uint64_t currentTime = SDL_GetTicks();
        caretUpdate(app->appRenderer->caretState, currentTime);

        // Caret blinking requires continuous redraw
        if (app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
            app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
            app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
            app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
            app->appRenderer->needsRedraw = true;
        }

        // Recreate swapchain if framebuffer was resized
        if (app->framebufferResized) {
            app->framebufferResized = false;
            recreateSwapChain(app);
            app->appRenderer->needsRedraw = true;
        }

        // Render if needed
        if (app->appRenderer->needsRedraw) {
            updateView(app);
            app->appRenderer->needsRedraw = false;

            drawFrame(app);
        }

        SDL_Delay(16); // ~60 FPS
    }

    // Cleanup
    appRendererDestroy(app->appRenderer);
}