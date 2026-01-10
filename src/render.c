#include "view.h"
#include <string.h>

void renderText(EditorState *state, const char *text, int x, int y,
                uint32_t color, bool highlight) {
    if (!text || strlen(text) == 0) {
        text = " "; // Render at least a space for empty lines
    }

    SDL_Color sdlColor;
    sdlColor.r = (color >> 24) & 0xFF;
    sdlColor.g = (color >> 16) & 0xFF;
    sdlColor.b = (color >> 8) & 0xFF;
    sdlColor.a = color & 0xFF;

    // Render highlight background if needed
    if (highlight) {
        SDL_FRect rect;
        int textWidth, textHeight;
        TTF_GetStringSize(state->font, text, 0, &textWidth, &textHeight);

        rect.x = x;
        rect.y = y;
        rect.w = textWidth + 8; // Add padding
        rect.h = state->fontHeight;

        SDL_SetRenderDrawColor(state->renderer,
                             (COLOR_GREEN >> 24) & 0xFF,
                             (COLOR_GREEN >> 16) & 0xFF,
                             (COLOR_GREEN >> 8) & 0xFF,
                             COLOR_GREEN & 0xFF);
        SDL_RenderFillRect(state->renderer, &rect);
    }

    SDL_Surface *surface = TTF_RenderText_Blended(state->font, text, 0, sdlColor);
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

void renderLine(EditorState *state, FfonElement *elem, const IdArray *id,
                int indent, int *yPos) {
    if (*yPos < -state->fontHeight || *yPos > 720) {
        // Skip off-screen lines
        *yPos += state->fontHeight;
        return;
    }

    int x = 50 + indent * INDENT_CHARS * state->charWidth;
    bool isCurrent = idArrayEqual(id, &state->currentId);

    if (elem->type == FFON_STRING) {
        uint32_t color = COLOR_TEXT;
        renderText(state, elem->data.string, x, *yPos, color, isCurrent);
    } else {
        // Render key with colon
        char keyWithColon[MAX_LINE_LENGTH];
        snprintf(keyWithColon, sizeof(keyWithColon), "%s:", elem->data.object->key);

        uint32_t color = COLOR_TEXT;
        renderText(state, keyWithColon, x, *yPos, color, isCurrent);
    }

    *yPos += state->fontHeight;

    // Recursively render children if object
    if (elem->type == FFON_OBJECT) {
        IdArray childId;
        idArrayCopy(&childId, id);
        idArrayPush(&childId, 0);

        for (int i = 0; i < elem->data.object->count; i++) {
            childId.ids[childId.depth - 1] = i;
            renderLine(state, elem->data.object->elements[i], &childId,
                       indent + 1, yPos);
        }
    }
}

void renderLeftPanel(EditorState *state) {
    int yPos = 40; // Start below header

    if (state->ffonCount == 0) {
        renderText(state, "", 50, yPos, COLOR_TEXT, true);
        return;
    }

    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 0);

    for (int i = 0; i < state->ffonCount; i++) {
        id.ids[0] = i;
        renderLine(state, state->ffon[i], &id, 0, &yPos);
    }
}

void renderRightPanel(EditorState *state) {
    int yPos = 40;

    // Render filter input
    char filterText[MAX_LINE_LENGTH];
    snprintf(filterText, sizeof(filterText), "filter: %s", state->inputBuffer);
    renderText(state, filterText, 50, yPos, COLOR_TEXT, false);
    yPos += state->fontHeight * 2;

    // Render list items
    ListItem *list = state->filteredListCount > 0 ?
                     state->filteredListRight : state->totalListRight;
    int count = state->filteredListCount > 0 ?
                state->filteredListCount : state->totalListCount;

    for (int i = 0; i < count; i++) {
        bool isSelected = (i == state->listIndex);

        // Render radio button indicator
        const char *indicator = isSelected ? "●" : "○";
        renderText(state, indicator, 50, yPos, COLOR_ORANGE, false);

        // Render text
        renderText(state, list[i].value, 80, yPos, COLOR_TEXT, isSelected);

        yPos += state->fontHeight;
    }
}

void updateView(EditorState *state) {
    // Clear screen
    SDL_SetRenderDrawColor(state->renderer,
                          (COLOR_BG >> 24) & 0xFF,
                          (COLOR_BG >> 16) & 0xFF,
                          (COLOR_BG >> 8) & 0xFF,
                          COLOR_BG & 0xFF);
    SDL_RenderClear(state->renderer);

    // Render header
    char header[256];
    snprintf(header, sizeof(header), "%s", coordinateToString(state->currentCoordinate));
    renderText(state, header, 50, 10, COLOR_TEXT, false);

    // Render error message if any
    if (state->errorMessage[0] != '\0') {
        renderText(state, state->errorMessage, 400, 10, COLOR_RED, false);
    }

    // Draw header separator
    SDL_SetRenderDrawColor(state->renderer,
                          (COLOR_BORDER >> 24) & 0xFF,
                          (COLOR_BORDER >> 16) & 0xFF,
                          (COLOR_BORDER >> 8) & 0xFF,
                          COLOR_BORDER & 0xFF);
    SDL_RenderLine(state->renderer, 0, 35, 1280, 35);

    // Render appropriate panel
    if (state->currentCoordinate == COORDINATE_RIGHT_INFO ||
        state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        state->currentCoordinate == COORDINATE_RIGHT_FIND) {
        renderRightPanel(state);
    } else {
        renderLeftPanel(state);
    }

    // Present
    SDL_RenderPresent(state->renderer);
}
