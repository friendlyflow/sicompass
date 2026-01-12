#include "view.h"
#include <string.h>

// Helper function to convert COLOR_ format to vec3
static void colorToVec3(uint32_t color, vec3 outVec) {
    outVec[0] = ((color >> 24) & 0xFF) / 255.0f;
    outVec[1] = ((color >> 16) & 0xFF) / 255.0f;
    outVec[2] = ((color >> 8) & 0xFF) / 255.0f;
}

// Helper function to convert COLOR_ format to vec4
static void colorToVec4(uint32_t color, vec4 outVec) {
    outVec[0] = ((color >> 24) & 0xFF) / 255.0f;
    outVec[1] = ((color >> 16) & 0xFF) / 255.0f;
    outVec[2] = ((color >> 8) & 0xFF) / 255.0f;
    outVec[3] = (color & 0xFF) / 255.0f;
}

void renderText(SiCompassApplication *app, const char *text, int x, int y,
                uint32_t color, bool highlight) {
    if (!text || strlen(text) == 0) {
        text = " "; // Render at least a space for empty lines
    }

    float scale = getTextScale(app, FONT_SIZE_PT);

    // Render highlight background if needed
    if (highlight) {
        vec4 bgColor;
        colorToVec4(COLOR_DARK_GREEN, bgColor);

        // Calculate text bounds
        float minX, minY, maxX, maxY;
        calculateTextBounds(app, text, (float)x, (float)y, scale, &minX, &minY, &maxX, &maxY);

        // Add padding
        minX -= TEXT_PADDING;
        minY -= TEXT_PADDING;
        maxX += TEXT_PADDING;
        maxY += TEXT_PADDING;

        float width = maxX - minX;
        float height = maxY - minY;

        // Use a reasonable corner radius
        float cornerRadius = 5.0f;

        prepareRectangle(app, minX, minY, width, height, bgColor, cornerRadius);
    }

    // Prepare text for rendering
    vec3 textColor;
    colorToVec3(color, textColor);
    prepareTextForRendering(app, text, (float)x, (float)y, scale, textColor);
}

void renderLine(SiCompassApplication *app, FfonElement *elem, const IdArray *id,
                int indent, int *yPos) {
    // Calculate scale and dimensions from font metrics
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    if (*yPos < -lineHeight || *yPos > 720) {
        // Skip off-screen lines
        *yPos += lineHeight;
        return;
    }

    // Get character width from font (using 'M' as em-width, monospace assumption)
    int charWidth = (int)getWidthEM(app, scale);
    int x = 50 + indent * INDENT_CHARS * charWidth;
    bool isCurrent = idArrayEqual(id, &app->appRenderer->currentId);

    if (elem->type == FFON_STRING) {
        uint32_t color = COLOR_TEXT;
        renderText(app, elem->data.string, x, *yPos, color, isCurrent);
    } else {
        // Render key with colon
        char keyWithColon[MAX_LINE_LENGTH];
        snprintf(keyWithColon, sizeof(keyWithColon), "%s:", elem->data.object->key);

        uint32_t color = COLOR_TEXT;
        renderText(app, keyWithColon, x, *yPos, color, isCurrent);
    }

    *yPos += lineHeight;

    // Recursively render children if object
    if (elem->type == FFON_OBJECT) {
        IdArray childId;
        idArrayCopy(&childId, id);
        idArrayPush(&childId, 0);

        for (int i = 0; i < elem->data.object->count; i++) {
            childId.ids[childId.depth - 1] = i;
            renderLine(app, elem->data.object->elements[i], &childId,
                       indent + 1, yPos);
        }
    }
}

void renderLeftPanel(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int yPos = 2 * getLineHeight(app, scale, TEXT_PADDING);

    if (app->appRenderer->ffonCount == 0) {
        renderText(app, "", 50, yPos, COLOR_TEXT, true);
        return;
    }

    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 0);

    for (int i = 0; i < app->appRenderer->ffonCount; i++) {
        id.ids[0] = i;
        renderLine(app, app->appRenderer->ffon[i], &id, 0, &yPos);
    }
}

void renderRightPanel(SiCompassApplication *app) {
    int yPos = 40;
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // Render filter input
    char filterText[MAX_LINE_LENGTH];
    snprintf(filterText, sizeof(filterText), "filter: %s", app->appRenderer->inputBuffer);
    renderText(app, filterText, 50, yPos, COLOR_TEXT, false);
    yPos += lineHeight * 2;

    // Render list items
    ListItem *list = app->appRenderer->filteredListCount > 0 ?
                     app->appRenderer->filteredListRight : app->appRenderer->totalListRight;
    int count = app->appRenderer->filteredListCount > 0 ?
                app->appRenderer->filteredListCount : app->appRenderer->totalListCount;

    for (int i = 0; i < count; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);

        // Render radio button indicator
        const char *indicator = isSelected ? "●" : "○";
        renderText(app, indicator, 50, yPos, COLOR_ORANGE, false);

        // Render text
        renderText(app, list[i].value, 80, yPos, COLOR_TEXT, isSelected);

        yPos += lineHeight;
    }
}

void updateView(SiCompassApplication *app) {
    // Note: Screen clearing is handled by the Vulkan rendering pipeline in drawFrame()

    // Begin text rendering for this frame
    beginTextRendering(app);

    // Render header
    float scale = getTextScale(app, FONT_SIZE_PT);
    char header[256];
    snprintf(header, sizeof(header), "%s", coordinateToString(app->appRenderer->currentCoordinate));
    renderText(app, header, 50, getLineHeight(app, scale, TEXT_PADDING), COLOR_TEXT, false);

    // Render error message if any
    if (app->appRenderer->errorMessage[0] != '\0') {
        renderText(app, app->appRenderer->errorMessage, 400, 10, COLOR_RED, false);
    }

    // TODO: Add line rendering support for header separator at y=35

    // Render appropriate panel
    if (app->appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        app->appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        app->appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        renderRightPanel(app);
    } else {
        renderLeftPanel(app);
    }

    // The actual drawing to the screen happens in drawFrame() which calls
    // drawBackground() and drawText() with the prepared vertices
}
