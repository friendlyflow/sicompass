#include "view.h"
#include "provider.h"
#include <filebrowser_provider.h>
#include <string.h>

void mainLoop(SiCompassApplication* app) {
    // Initialize app renderer
    app = appRendererCreate(app);
    if (!app->appRenderer) {
        fprintf(stderr, "Failed to create editor state\n");
        return;
    }

    // Register providers
    Provider *fbProvider = filebrowserGetProvider();
    providerRegister(fbProvider);
    providerInitAll();

    // Get initial element from provider
    FfonElement *fileBrowserElement = providerGetInitialElement(fbProvider);
    if (!fileBrowserElement) {
        fprintf(stderr, "Failed to get initial element from filebrowser provider\n");
        appRendererDestroy(app->appRenderer);
        return;
    }

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
                    if (event.key.windowID != app->window_id) break;
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
                    if (event.text.windowID != app->window_id) break;
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
                    if (event.window.windowID != app->window_id) break;
                    app->framebufferResized = true;
                    app->appRenderer->needsRedraw = true;
                    break;

                case SDL_EVENT_WINDOW_FOCUS_GAINED:
                    if (event.window.windowID != app->window_id) break;
                    accesskitUpdateWindowFocus(app->appRenderer, true);
                    break;

                case SDL_EVENT_WINDOW_FOCUS_LOST:
                    if (event.window.windowID != app->window_id) break;
                    accesskitUpdateWindowFocus(app->appRenderer, false);
                    break;

                default:
                    // Handle custom accessibility events
                    if (event.type == app->user_event) {
                        if (event.user.windowID != app->window_id) break;
                        // Process AccessKit action request
                        accesskit_action_request *request = (accesskit_action_request *)event.user.data1;
                        if (request) {
                            accesskit_action action = accesskit_action_request_action(request);
                            accesskit_node_id target = accesskit_action_request_target(request);

                            // Check if target is a list item (ID >= 100)
                            if (target >= 100) {
                                int index = target - 100;

                                switch (action) {
                                    case ACCESSKIT_ACTION_CLICK:
                                        // Activate item (like pressing Enter)
                                        app->appRenderer->listIndex = index;
                                        handleEnter(app->appRenderer, HISTORY_NONE);
                                        break;

                                    case ACCESSKIT_ACTION_FOCUS:
                                        // Navigate to item
                                        app->appRenderer->listIndex = index;
                                        accesskitSpeakCurrentItem(app->appRenderer);
                                        break;

                                    case ACCESSKIT_ACTION_SCROLL_DOWN:
                                        handleDown(app->appRenderer);  // j key - next item
                                        break;

                                    case ACCESSKIT_ACTION_SCROLL_UP:
                                        handleUp(app->appRenderer);    // k key - previous item
                                        break;

                                    case ACCESSKIT_ACTION_SCROLL_LEFT:
                                        handleLeft(app->appRenderer);  // h key - go to parent
                                        break;

                                    case ACCESSKIT_ACTION_SCROLL_RIGHT:
                                        handleRight(app->appRenderer); // l key - enter item
                                        break;

                                    default:
                                        break;
                                }
                                app->appRenderer->needsRedraw = true;
                            }

                            accesskit_action_request_free(request);
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