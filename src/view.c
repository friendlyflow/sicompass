#include "view.h"

void updateView(SiCompassApplication* app) {
    while (app->running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event->type == SDL_EVENT_QUIT) {
                app->running = false;
            }
            else if (event->type == SDL_EVENT_WINDOW_RESIZED) {
                app->framebufferResized = true;
            }

            char* text = "Hello Vulkan!";

            // Reset text rendering for this frame
            beginTextRendering(app);

            // Prepare background with rounded corners
            vec4 bgColor = {0.110f, 0.267f, 0.078f, 1.0f};
            prepareBackgroundForText(app, text, 50.0f, 50.0f, 0.25f, bgColor, 5.0f, 10.0f);

            // Prepare text on top (scales adjusted for 48pt base size)
            prepareTextForRendering(app, text, 50.0f, 50.0f, 0.25f, (vec3){0.753f, 0.925f, 0.722f});
            prepareTextForRendering(app, "Small Text", 200.0f, 50.0f, 0.125f, (vec3){0.753f, 0.925f, 0.722f});
            prepareTextForRendering(app, "Large Text", 200.0f, 100.0f, 0.5f, (vec3){0.753f, 0.925f, 0.722f});
            drawFrame(app);
        }
    }
}