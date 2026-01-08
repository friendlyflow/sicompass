#include "view.h"
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char *argv[]) {
    // Initialize editor state
    EditorState *state = editorStateCreate();
    if (!state) {
        fprintf(stderr, "Failed to create editor state\n");
        return 1;
    }

    // Initialize SDL
    if (!initSdl(state)) {
        fprintf(stderr, "Failed to initialize SDL\n");
        editorStateDestroy(state);
        return 1;
    }

    // Load JSON file
    const char *jsonFile = "src/json/sf.json";
    if (argc > 1) {
        jsonFile = argv[1];
    }

    if (!loadJsonFile(state, jsonFile)) {
        fprintf(stderr, "Failed to load JSON file: %s\n", jsonFile);
        cleanupSdl(state);
        editorStateDestroy(state);
        return 1;
    }

    // Initialize currentId
    idArrayInit(&state->currentId);
    idArrayPush(&state->currentId, 0);

    // Set initial coordinate
    state->currentCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    state->previousCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;

    // Initial render
    state->needsRedraw = true;
    updateView(state);

    // Main event loop
    SDL_Event event;
    while (state->running) {
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
                case SDL_EVENT_QUIT:
                    state->running = false;
                    break;

                case SDL_EVENT_KEY_DOWN:
                    handleKeys(state, &event);
                    break;

                case SDL_EVENT_TEXT_INPUT:
                    if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT ||
                        state->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT ||
                        state->currentCoordinate == COORDINATE_RIGHT_INFO ||
                        state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
                        state->currentCoordinate == COORDINATE_RIGHT_FIND) {
                        handleInput(state, event.text.text);
                    }
                    break;

                case SDL_EVENT_WINDOW_RESIZED:
                case SDL_EVENT_WINDOW_EXPOSED:
                    state->needsRedraw = true;
                    break;
            }
        }

        // Render if needed
        if (state->needsRedraw) {
            updateView(state);
            state->needsRedraw = false;
        }

        SDL_Delay(16); // ~60 FPS
    }

    // Cleanup
    cleanupSdl(state);
    editorStateDestroy(state);

    return 0;
}
