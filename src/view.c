#include "view.h"

void mainLoop(SiCompassApplication* app) {
    // Initialize app renderer
    app = appRendererCreate(app);
    if (!app->appRenderer) {
        fprintf(stderr, "Failed to create editor state\n");
        return;
    }

    // Load JSON file
    const char *jsonFile = "src/json/sf.json";
    int count;
    FfonElement **elements = loadJsonFileToElements(jsonFile, &count);
    if (!elements) {
        fprintf(stderr, "Failed to load JSON file: %s\n", jsonFile);
        appRendererDestroy(app->appRenderer);
        return;
    }

    // Assign to app renderer
    app->appRenderer->ffon = elements;
    app->appRenderer->ffonCount = count;
    app->appRenderer->ffonCapacity = count > 0 ? count : 1;

    // Initialize current_id
    idArrayInit(&app->appRenderer->currentId);
    idArrayPush(&app->appRenderer->currentId, 0);
    
    // Set initial coordinate
    app->appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    app->appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;
    
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
                        app->appRenderer->currentCoordinate == COORDINATE_LIST ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_FIND) {
                        SDL_StartTextInput(app->window);
                    } else {
                        SDL_StopTextInput(app->window);
                    }
                    break;

                case SDL_EVENT_TEXT_INPUT:
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_LIST ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_FIND) {
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
            app->appRenderer->currentCoordinate == COORDINATE_LIST ||
            app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            app->appRenderer->currentCoordinate == COORDINATE_FIND) {
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