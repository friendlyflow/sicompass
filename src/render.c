#include "view.h"
#include <string.h>

void renderText(AppRenderer *appRenderer, const char *text, int x, int y,
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
        TTF_GetStringSize(appRenderer->font, text, 0, &textWidth, &textHeight);

        rect.x = x;
        rect.y = y;
        rect.w = textWidth + 8; // Add padding
        rect.h = appRenderer->fontHeight;

        SDL_SetRenderDrawColor(appRenderer->renderer,
                             (COLOR_GREEN >> 24) & 0xFF,
                             (COLOR_GREEN >> 16) & 0xFF,
                             (COLOR_GREEN >> 8) & 0xFF,
                             COLOR_GREEN & 0xFF);
        SDL_RenderFillRect(appRenderer->renderer, &rect);
    }

    SDL_Surface *surface = TTF_RenderText_Blended(appRenderer->font, text, 0, sdlColor);
    if (!surface) return;

    SDL_Texture *texture = SDL_CreateTextureFromSurface(appRenderer->renderer, surface);
    SDL_DestroySurface(surface);

    if (!texture) return;

    SDL_FRect dest;
    dest.x = x + 4; // Add padding
    dest.y = y;
    SDL_GetTextureSize(texture, &dest.w, &dest.h);

    SDL_RenderTexture(appRenderer->renderer, texture, NULL, &dest);
    SDL_DestroyTexture(texture);
}

void renderLine(AppRenderer *appRenderer, FfonElement *elem, const IdArray *id,
                int indent, int *yPos) {
    if (*yPos < -appRenderer->fontHeight || *yPos > 720) {
        // Skip off-screen lines
        *yPos += appRenderer->fontHeight;
        return;
    }

    int x = 50 + indent * INDENT_CHARS * appRenderer->charWidth;
    bool isCurrent = idArrayEqual(id, &appRenderer->currentId);

    if (elem->type == FFON_STRING) {
        uint32_t color = COLOR_TEXT;
        renderText(appRenderer, elem->data.string, x, *yPos, color, isCurrent);
    } else {
        // Render key with colon
        char keyWithColon[MAX_LINE_LENGTH];
        snprintf(keyWithColon, sizeof(keyWithColon), "%s:", elem->data.object->key);

        uint32_t color = COLOR_TEXT;
        renderText(appRenderer, keyWithColon, x, *yPos, color, isCurrent);
    }

    *yPos += appRenderer->fontHeight;

    // Recursively render children if object
    if (elem->type == FFON_OBJECT) {
        IdArray childId;
        idArrayCopy(&childId, id);
        idArrayPush(&childId, 0);

        for (int i = 0; i < elem->data.object->count; i++) {
            childId.ids[childId.depth - 1] = i;
            renderLine(appRenderer, elem->data.object->elements[i], &childId,
                       indent + 1, yPos);
        }
    }
}

void renderLeftPanel(AppRenderer *appRenderer) {
    int yPos = 40; // Start below header

    if (appRenderer->ffonCount == 0) {
        renderText(appRenderer, "", 50, yPos, COLOR_TEXT, true);
        return;
    }

    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 0);

    for (int i = 0; i < appRenderer->ffonCount; i++) {
        id.ids[0] = i;
        renderLine(appRenderer, appRenderer->ffon[i], &id, 0, &yPos);
    }
}

void renderRightPanel(AppRenderer *appRenderer) {
    int yPos = 40;

    // Render filter input
    char filterText[MAX_LINE_LENGTH];
    snprintf(filterText, sizeof(filterText), "filter: %s", appRenderer->inputBuffer);
    renderText(appRenderer, filterText, 50, yPos, COLOR_TEXT, false);
    yPos += appRenderer->fontHeight * 2;

    // Render list items
    ListItem *list = appRenderer->filteredListCount > 0 ?
                     appRenderer->filteredListRight : appRenderer->totalListRight;
    int count = appRenderer->filteredListCount > 0 ?
                appRenderer->filteredListCount : appRenderer->totalListCount;

    for (int i = 0; i < count; i++) {
        bool isSelected = (i == appRenderer->listIndex);

        // Render radio button indicator
        const char *indicator = isSelected ? "●" : "○";
        renderText(appRenderer, indicator, 50, yPos, COLOR_ORANGE, false);

        // Render text
        renderText(appRenderer, list[i].value, 80, yPos, COLOR_TEXT, isSelected);

        yPos += appRenderer->fontHeight;
    }
}

void updateView(AppRenderer *appRenderer) {
    // Clear screen
    SDL_SetRenderDrawColor(appRenderer->renderer,
                          (COLOR_BG >> 24) & 0xFF,
                          (COLOR_BG >> 16) & 0xFF,
                          (COLOR_BG >> 8) & 0xFF,
                          COLOR_BG & 0xFF);
    SDL_RenderClear(appRenderer->renderer);

    // Render header
    char header[256];
    snprintf(header, sizeof(header), "%s", coordinateToString(appRenderer->currentCoordinate));
    renderText(appRenderer, header, 50, 10, COLOR_TEXT, false);

    // Render error message if any
    if (appRenderer->errorMessage[0] != '\0') {
        renderText(appRenderer, appRenderer->errorMessage, 400, 10, COLOR_RED, false);
    }

    // Draw header separator
    SDL_SetRenderDrawColor(appRenderer->renderer,
                          (COLOR_BORDER >> 24) & 0xFF,
                          (COLOR_BORDER >> 16) & 0xFF,
                          (COLOR_BORDER >> 8) & 0xFF,
                          COLOR_BORDER & 0xFF);
    SDL_RenderLine(appRenderer->renderer, 0, 35, 1280, 35);

    // Render appropriate panel
    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        renderRightPanel(appRenderer);
    } else {
        renderLeftPanel(appRenderer);
    }

    // Present
    SDL_RenderPresent(appRenderer->renderer);
}
