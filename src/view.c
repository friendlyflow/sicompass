#include "view.h"
#include <filebrowser.h>
#include <string.h>

// Filebrowser provider callback
static FfonElement** filebrowserFetchCallback(AppRenderer *appRenderer, const char *parent_key, int *out_count) {
    (void)parent_key;  // We use currentUri instead
    // currentUri already has the full path built by providerUriAppend
    return filebrowserListDirectory(appRenderer->currentUri, false, out_count);
}

void mainLoop(SiCompassApplication* app) {
    // Initialize app renderer
    app = appRendererCreate(app);
    if (!app->appRenderer) {
        fprintf(stderr, "Failed to create editor state\n");
        return;
    }

    // Set up filebrowser provider
    providerSetFetchCallback(filebrowserFetchCallback);

    // Initialize URI to root and load initial directory listing
    strncpy(app->appRenderer->currentUri, "/", MAX_URI_LENGTH);
    int count = 0;
    FfonElement **elements = filebrowserListDirectory("/", false, &count);
    if (!elements || count == 0) {
        fprintf(stderr, "Failed to load directory listing for /\n");
        if (elements) free(elements);
        appRendererDestroy(app->appRenderer);
        return;
    }

    // Create top-level "file browser" object containing the file system
    FfonElement *fileBrowserElement = ffonElementCreateObject("file browser");
    FfonObject *fileBrowserObj = fileBrowserElement->data.object;
    for (int i = 0; i < count; i++) {
        ffonObjectAddElement(fileBrowserObj, elements[i]);
    }
    free(elements);  // Free the array, elements are now owned by fileBrowserObj

    // Create root array with just the file browser object
    app->appRenderer->ffon = malloc(sizeof(FfonElement*));
    app->appRenderer->ffon[0] = fileBrowserElement;
    app->appRenderer->ffonCount = 1;
    app->appRenderer->ffonCapacity = 1;

    // Initialize current_id - start at "file browser" object
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
                    handleKeys(app->appRenderer, &event);
                    // Enable/disable text input based on current mode
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                        SDL_StartTextInput(app->window);
                    } else {
                        SDL_StopTextInput(app->window);
                    }
                    break;

                case SDL_EVENT_TEXT_INPUT:
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                        handleInput(app->appRenderer, event.text.text);
                    }
                    break;

                case SDL_EVENT_WINDOW_RESIZED:
                case SDL_EVENT_WINDOW_MAXIMIZED:
                case SDL_EVENT_WINDOW_EXPOSED:
                    app->framebufferResized = true;
                    app->appRenderer->needsRedraw = true;
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