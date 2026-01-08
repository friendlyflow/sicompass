#include "sfon_editor.h"
#include <string.h>

void render_text(EditorState *state, const char *text, int x, int y, 
                uint32_t color, bool highlight) {
    if (!text || strlen(text) == 0) {
        text = " "; // Render at least a space for empty lines
    }
    
    SDL_Color sdl_color;
    sdl_color.r = (color >> 24) & 0xFF;
    sdl_color.g = (color >> 16) & 0xFF;
    sdl_color.b = (color >> 8) & 0xFF;
    sdl_color.a = color & 0xFF;
    
    // Render highlight background if needed
    if (highlight) {
        SDL_FRect rect;
        int text_width, text_height;
        TTF_GetStringSize(state->font, text, 0, &text_width, &text_height);
        
        rect.x = x;
        rect.y = y;
        rect.w = text_width + 8; // Add padding
        rect.h = state->font_height;
        
        SDL_SetRenderDrawColor(state->renderer, 
                             (COLOR_GREEN >> 24) & 0xFF,
                             (COLOR_GREEN >> 16) & 0xFF,
                             (COLOR_GREEN >> 8) & 0xFF,
                             COLOR_GREEN & 0xFF);
        SDL_RenderFillRect(state->renderer, &rect);
    }
    
    SDL_Surface *surface = TTF_RenderText_Blended(state->font, text, 0, sdl_color);
    if (!surface) return;
    
    SDL_Texture *texture = SDL_CreateTextureFromSurface(state->renderer, surface);
    SDL_DestroySurface(surface);
    
    if (!texture) return;
    
    SDL_FRect dest;
    dest.x = x + 4; // Add padding
    dest.y = y;
    SDL_GetTextureSize(texture, &dest.w, &dest.h);
    
    SDL_RenderTexture(state->renderer, texture, NULL, &dest);
    SDL_DestroyTexture(texture);
}

void render_line(EditorState *state, SfonElement *elem, const IdArray *id, 
                int indent, int *y_pos) {
    if (*y_pos < -state->font_height || *y_pos > 720) {
        // Skip off-screen lines
        *y_pos += state->font_height;
        return;
    }
    
    int x = 50 + indent * INDENT_CHARS * state->char_width;
    bool is_current = id_array_equal(id, &state->current_id);
    
    if (elem->type == SFON_STRING) {
        uint32_t color = COLOR_TEXT;
        render_text(state, elem->data.string, x, *y_pos, color, is_current);
    } else {
        // Render key with colon
        char key_with_colon[MAX_LINE_LENGTH];
        snprintf(key_with_colon, sizeof(key_with_colon), "%s:", elem->data.object->key);
        
        uint32_t color = COLOR_TEXT;
        render_text(state, key_with_colon, x, *y_pos, color, is_current);
    }
    
    *y_pos += state->font_height;
    
    // Recursively render children if object
    if (elem->type == SFON_OBJECT) {
        IdArray child_id;
        id_array_copy(&child_id, id);
        id_array_push(&child_id, 0);
        
        for (int i = 0; i < elem->data.object->count; i++) {
            child_id.ids[child_id.depth - 1] = i;
            render_line(state, elem->data.object->elements[i], &child_id, 
                       indent + 1, y_pos);
        }
    }
}

void render_left_panel(EditorState *state) {
    int y_pos = 40; // Start below header
    
    if (state->sfon_count == 0) {
        render_text(state, "", 50, y_pos, COLOR_TEXT, true);
        return;
    }
    
    IdArray id;
    id_array_init(&id);
    id_array_push(&id, 0);
    
    for (int i = 0; i < state->sfon_count; i++) {
        id.ids[0] = i;
        render_line(state, state->sfon[i], &id, 0, &y_pos);
    }
}

void render_right_panel(EditorState *state) {
    int y_pos = 40;
    
    // Render filter input
    char filter_text[MAX_LINE_LENGTH];
    snprintf(filter_text, sizeof(filter_text), "filter: %s", state->input_buffer);
    render_text(state, filter_text, 50, y_pos, COLOR_TEXT, false);
    y_pos += state->font_height * 2;
    
    // Render list items
    ListItem *list = state->filtered_list_count > 0 ? 
                     state->filtered_list_right : state->total_list_right;
    int count = state->filtered_list_count > 0 ? 
                state->filtered_list_count : state->total_list_count;
    
    for (int i = 0; i < count; i++) {
        bool is_selected = (i == state->list_index);
        
        // Render radio button indicator
        const char *indicator = is_selected ? "●" : "○";
        render_text(state, indicator, 50, y_pos, COLOR_ORANGE, false);
        
        // Render text
        render_text(state, list[i].value, 80, y_pos, COLOR_TEXT, is_selected);
        
        y_pos += state->font_height;
    }
}

void update_view(EditorState *state) {
    // Clear screen
    SDL_SetRenderDrawColor(state->renderer,
                          (COLOR_BG >> 24) & 0xFF,
                          (COLOR_BG >> 16) & 0xFF,
                          (COLOR_BG >> 8) & 0xFF,
                          COLOR_BG & 0xFF);
    SDL_RenderClear(state->renderer);
    
    // Render header
    char header[256];
    snprintf(header, sizeof(header), "%s", coordinate_to_string(state->current_coordinate));
    render_text(state, header, 50, 10, COLOR_TEXT, false);
    
    // Render error message if any
    if (state->error_message[0] != '\0') {
        render_text(state, state->error_message, 400, 10, COLOR_RED, false);
    }
    
    // Draw header separator
    SDL_SetRenderDrawColor(state->renderer,
                          (COLOR_BORDER >> 24) & 0xFF,
                          (COLOR_BORDER >> 16) & 0xFF,
                          (COLOR_BORDER >> 8) & 0xFF,
                          COLOR_BORDER & 0xFF);
    SDL_RenderLine(state->renderer, 0, 35, 1280, 35);
    
    // Render appropriate panel
    if (state->current_coordinate == COORDINATE_RIGHT_INFO ||
        state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
        state->current_coordinate == COORDINATE_RIGHT_FIND) {
        render_right_panel(state);
    } else {
        render_left_panel(state);
    }
    
    // Present
    SDL_RenderPresent(state->renderer);
}
