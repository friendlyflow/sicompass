#include "sfon_editor.h"
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char *argv[]) {
    // Initialize editor state
    EditorState *state = editor_state_create();
    if (!state) {
        fprintf(stderr, "Failed to create editor state\n");
        return 1;
    }
    
    // Initialize SDL
    if (!init_sdl(state)) {
        fprintf(stderr, "Failed to initialize SDL\n");
        editor_state_destroy(state);
        return 1;
    }
    
    // Load JSON file
    const char *json_file = "src/json/sf.json";
    if (argc > 1) {
        json_file = argv[1];
    }
    
    if (!load_json_file(state, json_file)) {
        fprintf(stderr, "Failed to load JSON file: %s\n", json_file);
        cleanup_sdl(state);
        editor_state_destroy(state);
        return 1;
    }
    
    // Initialize current_id
    id_array_init(&state->current_id);
    id_array_push(&state->current_id, 0);
    
    // Set initial coordinate
    state->current_coordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    state->previous_coordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    
    // Initial render
    state->needs_redraw = true;
    update_view(state);
    
    // Main event loop
    SDL_Event event;
    while (state->running) {
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
                case SDL_EVENT_QUIT:
                    state->running = false;
                    break;
                    
                case SDL_EVENT_KEY_DOWN:
                    handle_keys(state, &event);
                    break;
                    
                case SDL_EVENT_TEXT_INPUT:
                    if (state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT ||
                        state->current_coordinate == COORDINATE_LEFT_VISITOR_INSERT ||
                        state->current_coordinate == COORDINATE_RIGHT_INFO ||
                        state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
                        state->current_coordinate == COORDINATE_RIGHT_FIND) {
                        handle_input(state, event.text.text);
                    }
                    break;
                    
                case SDL_EVENT_WINDOW_RESIZED:
                case SDL_EVENT_WINDOW_EXPOSED:
                    state->needs_redraw = true;
                    break;
            }
        }
        
        // Render if needed
        if (state->needs_redraw) {
            update_view(state);
            state->needs_redraw = false;
        }
        
        SDL_Delay(16); // ~60 FPS
    }
    
    // Cleanup
    cleanup_sdl(state);
    editor_state_destroy(state);
    
    return 0;
}
