#include "view.h"

void startApp(SiCompassApplication* app) {
    // Initialize app renderer
    app = appRendererCreate(app);
    if (!app->appRenderer) {
        fprintf(stderr, "Failed to create editor state\n");
        return;
    }

    // Initialize current_id
    idArrayInit(&app->appRenderer->currentId);
    idArrayPush(&app->appRenderer->currentId, 0);
    
    // Set initial coordinate
    app->appRenderer->currentCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    app->appRenderer->previousCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    
    // Initial render
    app->appRenderer->needsRedraw = true;
    updateView(app->appRenderer);

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
                    break;

                case SDL_EVENT_TEXT_INPUT:
                    if (app->appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
                        app->appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
                        handleInput(app->appRenderer, event.text.text);
                    }
                    break;

                case SDL_EVENT_WINDOW_RESIZED:
                case SDL_EVENT_WINDOW_EXPOSED:
                    app->framebufferResized = true;
                    app->appRenderer->needsRedraw = true;
                    break;
            }
        }

        // Render if needed
        if (app->appRenderer->needsRedraw) {
            updateView(app->appRenderer);
            app->appRenderer->needsRedraw = false;
            
            // char* text = "Hello Vulkan!";

            // // Reset text rendering for this frame
            // beginTextRendering(app);

            // // Prepare background with rounded corners
            // vec4 bgColor = {0.110f, 0.267f, 0.078f, 1.0f};
            // prepareBackgroundForText(app, text, 50.0f, 50.0f, 0.25f, bgColor, 5.0f, 10.0f);

            // // Prepare text on top (scales adjusted for 48pt base size)
            // prepareTextForRendering(app, text, 50.0f, 50.0f, 0.25f, (vec3){0.753f, 0.925f, 0.722f});
            // prepareTextForRendering(app, "Small Text", 200.0f, 50.0f, 0.125f, (vec3){0.753f, 0.925f, 0.722f});
            // prepareTextForRendering(app, "Large Text", 200.0f, 100.0f, 0.5f, (vec3){0.753f, 0.925f, 0.722f});

            drawFrame(app);
            // app->appRenderer->needsRedraw = false;
        }

        SDL_Delay(16); // ~60 FPS
    }
}