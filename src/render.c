#include "view.h"
#include <filebrowser.h>
#include <string.h>

// AccessKit node IDs
#define ACCESSKIT_ROOT_ID 1
#define ACCESSKIT_LIVE_REGION_ID 2

// Callback for AccessKit activation - returns initial tree
static struct accesskit_tree_update* accesskitActivationHandler(void *userdata) {
    // Create initial tree with root window and live region
    struct accesskit_tree_update *update = accesskit_tree_update_with_capacity_and_focus(2, ACCESSKIT_ROOT_ID);

    // Create root node (window)
    struct accesskit_node *root = accesskit_node_new(ACCESSKIT_ROLE_WINDOW);
    accesskit_node_set_label(root, "Silicon's Compass");
    accesskit_node_id children[] = {ACCESSKIT_LIVE_REGION_ID};
    accesskit_node_set_children(root, 1, children);
    accesskit_tree_update_push_node(update, ACCESSKIT_ROOT_ID, root);

    // Create live region for announcements
    struct accesskit_node *liveRegion = accesskit_node_new(ACCESSKIT_ROLE_LABEL);
    accesskit_node_set_live(liveRegion, ACCESSKIT_LIVE_POLITE);
    accesskit_node_set_label(liveRegion, "");
    accesskit_tree_update_push_node(update, ACCESSKIT_LIVE_REGION_ID, liveRegion);

    // Set tree info
    struct accesskit_tree *tree = accesskit_tree_new(ACCESSKIT_ROOT_ID);
    accesskit_tree_set_toolkit_name(tree, "sicompass");
    accesskit_tree_set_toolkit_version(tree, "0.1");
    accesskit_tree_update_set_tree(update, tree);

    return update;
}

// Callback for AccessKit action requests
static void accesskitActionHandler(accesskit_action_request *request, void *userdata) {
    // Handle accessibility actions (click, focus, etc.)
    // For now, we just free the request
    accesskit_action_request_free(request);
}

// Callback for AccessKit deactivation
static void accesskitDeactivationHandler(void *userdata) {
    // Called when assistive technology disconnects
    // Nothing to do here for now
}

void accesskitInit(SiCompassApplication *app) {
    app->appRenderer->accesskitRootId = ACCESSKIT_ROOT_ID;
    app->appRenderer->accesskitLiveRegionId = ACCESSKIT_LIVE_REGION_ID;

    // Create platform-specific adapter
#if defined(__APPLE__)
    // macOS: AccessKit uses NSAccessibility under the hood
    // Note: macOS adapter requires window handle, which we don't have here
    // For now, disable accessibility on macOS until proper SDL3 integration
    app->appRenderer->accesskitAdapter = NULL;
#elif defined(_WIN32)
    // Windows: AccessKit uses UI Automation
    // Note: Windows adapter requires HWND, which we don't have here
    // For now, disable accessibility on Windows until proper SDL3 integration
    app->appRenderer->accesskitAdapter = NULL;
#else
    // Linux/Unix: AccessKit uses AT-SPI over D-Bus
    app->appRenderer->accesskitAdapter = accesskit_unix_adapter_new(
        accesskitActivationHandler,
        NULL,  // userdata for activation handler
        accesskitActionHandler,
        NULL,  // userdata for action handler
        accesskitDeactivationHandler,
        NULL   // userdata for deactivation handler
    );
#endif
}

void accesskitDestroy(AppRenderer *appRenderer) {
    if (appRenderer->accesskitAdapter) {
#if defined(__APPLE__)
        accesskit_macos_adapter_free(appRenderer->accesskitAdapter);
#elif defined(_WIN32)
        accesskit_windows_adapter_free(appRenderer->accesskitAdapter);
#else
        accesskit_unix_adapter_free(appRenderer->accesskitAdapter);
#endif
        appRenderer->accesskitAdapter = NULL;
    }
}

// Factory function for tree updates when speaking
static struct accesskit_tree_update* accesskitSpeakUpdateFactory(void *userdata) {
    const char *text = (const char *)userdata;

    struct accesskit_tree_update *update = accesskit_tree_update_with_focus(ACCESSKIT_ROOT_ID);

    // Update live region with new text
    struct accesskit_node *liveRegion = accesskit_node_new(ACCESSKIT_ROLE_LABEL);
    accesskit_node_set_live(liveRegion, ACCESSKIT_LIVE_POLITE);
    accesskit_node_set_label(liveRegion, text);
    accesskit_tree_update_push_node(update, ACCESSKIT_LIVE_REGION_ID, liveRegion);

    return update;
}

void accesskitSpeak(AppRenderer *appRenderer, const char *text) {
    if (!appRenderer->accesskitAdapter || !text) {
        return;
    }

    // Update the tree with new live region content (platform-specific)
#if defined(__APPLE__)
    accesskit_macos_adapter_update_if_active(
        appRenderer->accesskitAdapter,
        accesskitSpeakUpdateFactory,
        (void *)text
    );
#elif defined(_WIN32)
    accesskit_windows_adapter_update_if_active(
        appRenderer->accesskitAdapter,
        accesskitSpeakUpdateFactory,
        (void *)text
    );
#else
    accesskit_unix_adapter_update_if_active(
        appRenderer->accesskitAdapter,
        accesskitSpeakUpdateFactory,
        (void *)text
    );
#endif
}

int renderText(SiCompassApplication *app, const char *text, int x, int y,
               uint32_t color, bool highlight) {
    if (!text || strlen(text) == 0) {
        text = " "; // Render at least a space for empty lines
    }

    float scale = getTextScale(app, FONT_SIZE_PT);
    float charWidth = getWidthEM(app, scale);
    float maxWidth = charWidth * 120.0f; // Maximum line width: 120 characters
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // First pass: split text into lines and store them
    typedef struct {
        const char *start;
        size_t len;
    } LineInfo;

    LineInfo lines[1000]; // Support up to 1000 lines
    int lineCount = 0;

    const char *lineStart = text;

    while (*lineStart != '\0' && lineCount < 1000) {
        const char *lineEnd = lineStart;
        const char *lastSpace = NULL;

        // Find where to break this line
        const char *lastFit = lineStart;
        int currentY = y + lineCount * lineHeight;

        while (*lineEnd != '\0') {
            // Build substring from lineStart to lineEnd (inclusive)
            size_t testLen = lineEnd - lineStart + 1;
            if (testLen >= MAX_LINE_LENGTH) {
                testLen = MAX_LINE_LENGTH - 1;
            }

            char testText[MAX_LINE_LENGTH];
            strncpy(testText, lineStart, testLen);
            testText[testLen] = '\0';

            // Measure width
            float minX, minY, maxX, maxY;
            calculateTextBounds(app, testText, (float)x, (float)currentY, scale,
                              &minX, &minY, &maxX, &maxY);
            float width = maxX - minX;

            // If adding this character exceeds the limit
            if (width > maxWidth) {
                // Break at last space if we have one
                if (lastSpace != NULL && lastSpace > lineStart) {
                    lineEnd = lastSpace;
                } else {
                    // No suitable space - use last position that fit
                    lineEnd = lastFit;
                }
                break;
            }

            // Remember where spaces are
            if (*lineEnd == ' ') {
                lastSpace = lineEnd;
            }

            // Move to next character
            lineEnd++;

            // This character fit, so the next line would start here if we need to break
            lastFit = lineEnd;
        }

        // Extract line to render
        size_t lineLen = lineEnd - lineStart;
        if (lineLen == 0 && *lineStart != '\0') {
            lineLen = 1; // Take at least one character
            lineEnd = lineStart + 1;
        }

        if (lineLen >= MAX_LINE_LENGTH) {
            lineLen = MAX_LINE_LENGTH - 1;
        }

        // Store line info
        lines[lineCount].start = lineStart;
        lines[lineCount].len = lineLen;
        lineCount++;

        // Move to next line
        lineStart = lineEnd;

        // Skip trailing space if we broke at one
        if (*lineStart == ' ') {
            lineStart++;
        }
    }

    // Second pass: calculate overall bounding box and render highlight if needed
    if (highlight && lineCount > 0) {
        float overallMinX = INFINITY;
        float overallMinY = INFINITY;
        float overallMaxX = -INFINITY;
        float overallMaxY = -INFINITY;

        for (int i = 0; i < lineCount; i++) {
            char lineText[MAX_LINE_LENGTH];
            strncpy(lineText, lines[i].start, lines[i].len);
            lineText[lines[i].len] = '\0';

            int currentY = y + i * lineHeight;
            float minX, minY, maxX, maxY;
            calculateTextBounds(app, lineText, (float)x, (float)currentY, scale,
                              &minX, &minY, &maxX, &maxY);

            if (minX < overallMinX) overallMinX = minX;
            if (minY < overallMinY) overallMinY = minY;
            if (maxX > overallMaxX) overallMaxX = maxX;
            if (maxY > overallMaxY) overallMaxY = maxY;
        }

        // Add padding and render single rectangle
        overallMinX -= TEXT_PADDING;
        overallMinY -= TEXT_PADDING;
        overallMaxX += TEXT_PADDING;
        overallMaxY += TEXT_PADDING;

        float width = overallMaxX - overallMinX;
        float height = overallMaxY - overallMinY;
        float cornerRadius = 5.0f;

        prepareRectangle(app, overallMinX, overallMinY, width, height, COLOR_DARK_GREEN, cornerRadius);
    }

    // Third pass: render all text lines
    int currentY = y;
    for (int i = 0; i < lineCount; i++) {
        char lineText[MAX_LINE_LENGTH];
        strncpy(lineText, lines[i].start, lines[i].len);
        lineText[lines[i].len] = '\0';

        if (lines[i].len > 0) {
            prepareTextForRendering(app, lineText, (float)x, (float)currentY, scale, color);
            currentY += lineHeight;
        }
    }

    return lineCount;
}

// void renderLine(SiCompassApplication *app, FfonElement *elem, const IdArray *id,
//                 int indent, int *yPos) {
//     // Calculate scale and dimensions from font metrics
//     float scale = getTextScale(app, FONT_SIZE_PT);
//     int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

//     if (*yPos < -lineHeight || *yPos > (int)app->swapChainExtent.height) {
//         // Skip off-screen lines
//         *yPos += lineHeight;
//         return;
//     }

//     // Get character width from font (using 'M' as em-width, monospace assumption)
//     int charWidth = (int)getWidthEM(app, scale);
//     int x = 50 + indent * INDENT_CHARS * charWidth;
//     bool isCurrent = idArrayEqual(id, &app->appRenderer->currentId);

//     // Store position of current element for caret rendering
//     if (isCurrent) {
//         app->appRenderer->currentElementX = x;
//         app->appRenderer->currentElementY = *yPos;
//         app->appRenderer->currentElementIsObject = (elem->type == FFON_OBJECT);
//     }

//     int linesRendered = 0;

//     if (elem->type == FFON_STRING) {
//         uint32_t color = COLOR_TEXT;
//         const char *displayText = elem->data.string;

//         // In insert mode, show inputBuffer for current element
//         if (isCurrent && (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
//                          app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT)) {
//             displayText = app->appRenderer->inputBuffer;
//         }

//         linesRendered = renderText(app, displayText, x, *yPos, color, isCurrent);

//         // Speak current element for accessibility
//         if (isCurrent) {
//             accesskitSpeak(app->appRenderer, displayText);
//         }
//     } else {
//         // Render key with colon
//         char keyWithColon[MAX_LINE_LENGTH];
//         const char *keyToRender = elem->data.object->key;

//         // In insert mode, inputBuffer already contains the colon
//         if (isCurrent && (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
//                          app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT)) {
//             strncpy(keyWithColon, app->appRenderer->inputBuffer, MAX_LINE_LENGTH - 1);
//             keyWithColon[MAX_LINE_LENGTH - 1] = '\0';
//         } else {
//             snprintf(keyWithColon, sizeof(keyWithColon), "%s:", keyToRender);
//         }

//         uint32_t color = COLOR_TEXT;
//         linesRendered = renderText(app, keyWithColon, x, *yPos, color, isCurrent);

//         // Speak current element for accessibility
//         if (isCurrent) {
//             accesskitSpeak(app->appRenderer, keyWithColon);
//         }
//     }

//     *yPos += lineHeight * linesRendered;

//     // Recursively render children if object
//     if (elem->type == FFON_OBJECT) {
//         IdArray childId;
//         idArrayCopy(&childId, id);
//         idArrayPush(&childId, 0);

//         for (int i = 0; i < elem->data.object->count; i++) {
//             childId.ids[childId.depth - 1] = i;
//             renderLine(app, elem->data.object->elements[i], &childId,
//                        indent + 1, yPos);
//         }
//     }
// }

// void renderHierarchy(SiCompassApplication *app) {
//     float scale = getTextScale(app, FONT_SIZE_PT);
//     int yPos = 2 * getLineHeight(app, scale, TEXT_PADDING);

//     if (app->appRenderer->ffonCount == 0) {
//         // When there are no elements, but we're in insert mode, show the input buffer
//         const char *displayText = "";
//         if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
//             app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
//             displayText = app->appRenderer->inputBuffer;

//             // Store position for caret rendering
//             app->appRenderer->currentElementX = 50;
//             app->appRenderer->currentElementY = yPos;
//             app->appRenderer->currentElementIsObject = false;
//         }
//         renderText(app, displayText, 50, yPos, COLOR_TEXT, true);
//         return;
//     }

//     IdArray id;
//     idArrayInit(&id);
//     idArrayPush(&id, 0);

//     for (int i = 0; i < app->appRenderer->ffonCount; i++) {
//         id.ids[0] = i;
//         renderLine(app, app->appRenderer->ffon[i], &id, 0, &yPos);
//     }
// }

void renderInteraction(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // Calculate indent as the actual width of 4 spaces using text bounds
    float minX, minY, maxX, maxY;
    calculateTextBounds(app, "    ", 0.0f, 0.0f, scale, &minX, &minY, &maxX, &maxY);
    int indent = (int)(maxX - minX);

    int yPos = lineHeight * 2;

    // Render parent element if we're not at root
    if (app->appRenderer->currentId.depth > 1) {
        IdArray parentId;
        idArrayCopy(&parentId, &app->appRenderer->currentId);
        idArrayPop(&parentId);

        int parentCount;
        FfonElement **parentArr = getFfonAtId(app->appRenderer->ffon, app->appRenderer->ffonCount, &parentId, &parentCount);
        if (parentArr && parentCount > 0) {
            int parentIdx = parentId.ids[parentId.depth - 1];
            if (parentIdx >= 0 && parentIdx < parentCount) {
                FfonElement *parentElem = parentArr[parentIdx];
                const char *parentText = (parentElem->type == FFON_OBJECT) ?
                    parentElem->data.object->key : parentElem->data.string;
                // Strip input tags from parent display
                char *strippedParent = filebrowserStripInputTags(parentText);
                renderText(app, strippedParent ? strippedParent : parentText, 50, yPos, COLOR_TEXT, false);
                free(strippedParent);
                yPos += lineHeight;
            }
        }
    }

    ListItem *list = app->appRenderer->filteredListCount > 0 ?
                     app->appRenderer->filteredListCurrentLayer : app->appRenderer->totalListCurrentLayer;
    int count = app->appRenderer->filteredListCount > 0 ?
                app->appRenderer->filteredListCount : app->appRenderer->totalListCount;

    if (!list || count == 0) {
        return;
    }

    bool inInsertMode = (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                         app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT);

    for (int i = 0; i < count; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Determine what text to display
        const char *displayText = list[i].value;

        // In insert mode, show inputBuffer for selected item
        if (isSelected && inInsertMode) {
            displayText = app->appRenderer->inputBuffer;

            // Store position for caret rendering
            app->appRenderer->currentElementX = itemX;
            app->appRenderer->currentElementY = itemYPos;
        }

        // Render text (may be multiple lines)
        int textLines = renderText(app, displayText, itemX, itemYPos, COLOR_TEXT, isSelected);

        // Speak selected item for accessibility
        if (isSelected) {
            accesskitSpeak(app->appRenderer, displayText);
        }

        yPos += lineHeight * textLines;
    }
}

void renderSimpleSearch(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // Calculate indent as the actual width of 4 spaces using text bounds
    float minX, minY, maxX, maxY;
    calculateTextBounds(app, "    ", 0.0f, 0.0f, scale, &minX, &minY, &maxX, &maxY);
    int indent = (int)(maxX - minX);

    int yPos = lineHeight * 2;

    // Render search input
    char searchText[MAX_LINE_LENGTH];
    snprintf(searchText, sizeof(searchText), "search: %s", app->appRenderer->inputBuffer);
    int linesRendered = renderText(app, searchText, 50, yPos, COLOR_TEXT, false);
    yPos += lineHeight * linesRendered;

    ListItem *list = app->appRenderer->filteredListCount > 0 ?
                     app->appRenderer->filteredListCurrentLayer : app->appRenderer->totalListCurrentLayer;
    int count = app->appRenderer->filteredListCount > 0 ?
                app->appRenderer->filteredListCount : app->appRenderer->totalListCount;

    for (int i = 0; i < count; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;

        // Render radio button indicator
        // const char *indicator = isSelected ? "●" : "○";
        // renderText(app, indicator, 50 + indent, itemYPos, COLOR_ORANGE, false);

        // Render text (may be multiple lines)
        int textLines = renderText(app, list[i].value, 50 + indent, itemYPos, COLOR_TEXT, isSelected);

        // Speak selected item for accessibility
        if (isSelected) {
            accesskitSpeak(app->appRenderer, list[i].value);
        }

        yPos += lineHeight * textLines;
    }
}

void updateView(SiCompassApplication *app) {
    // Note: Screen clearing is handled by the Vulkan rendering pipeline in drawFrame()

    // Begin text rendering for this frame
    beginTextRendering(app);

    // Begin rectangle rendering for this frame (resets rectangle count)
    beginRectangleRendering(app);

    // Render header
    float scale = getTextScale(app, FONT_SIZE_PT);
    char header[256];
    int lastId = app->appRenderer->currentId.ids[app->appRenderer->currentId.depth - 1];
    int maxId = getFfonMaxIdAtPath(app->appRenderer->ffon, app->appRenderer->ffonCount, &app->appRenderer->currentId);
    snprintf(header, sizeof(header), "%s layer: %d list: %d/%d",
             coordinateToString(app->appRenderer->currentCoordinate), app->appRenderer->currentId.depth - 1,
             lastId + 1, maxId + 1);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // Calculate text bounds for vertical centering
    float minX, minY, maxX, maxY;
    calculateTextBounds(app, header, 50.0f, (float)lineHeight, scale,
                          &minX, &minY, &maxX, &maxY);
    int headerHeight = (int)(maxY - minY);

    // Render line under header
    float headerWidth = (float)app->swapChainExtent.width;
    float lineThickness = 1.0f;
    prepareRectangle(app, 0.0f, (float)lineHeight, headerWidth, lineThickness, COLOR_DARK_GREY, 0.0f);

    renderText(app, header, (float)50, (float)headerHeight, COLOR_TEXT, false);

    // Render error message if any
    if (app->appRenderer->errorMessage[0] != '\0') {
        renderText(app, app->appRenderer->errorMessage, 400, 10, COLOR_RED, false);
    }

    // Render appropriate panel
    if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        renderSimpleSearch(app);
    } else {
        renderInteraction(app);
    }

    // Render caret for all modes at end of frame
    if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        // Caret in search field
        int searchTextYPos = lineHeight * 2;

        // Calculate X offset for "search: " prefix and get actual text Y position
        float minX, minY, maxX, maxY;
        calculateTextBounds(app, "search: ", 50.0f, (float)searchTextYPos, scale,
                          &minX, &minY, &maxX, &maxY);
        int searchPrefixWidth = (int)(maxX - minX);

        // Use minY from text bounds for proper vertical alignment
        caretRender(app, app->appRenderer->caretState,
                   app->appRenderer->inputBuffer,
                   50 + searchPrefixWidth, (int)minY,
                   app->appRenderer->cursorPosition,
                   COLOR_TEXT);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
               app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        // Caret in hierarchy editor
        int caretX = app->appRenderer->currentElementX;
        int caretY = app->appRenderer->currentElementY;

        // For objects, we render "inputBuffer:" so the caret needs to account for the colon
        // but the cursor position is within inputBuffer only
        const char *textForCaret = app->appRenderer->inputBuffer;

        // Get proper Y alignment from text bounds
        float minX, minY, maxX, maxY;
        calculateTextBounds(app, " ", (float)caretX, (float)caretY, scale,
                          &minX, &minY, &maxX, &maxY);
        caretY = (int)minY;

        caretRender(app, app->appRenderer->caretState,
                   textForCaret,
                   caretX, caretY,
                   app->appRenderer->cursorPosition,
                   COLOR_TEXT);
    }

    // The actual drawing to the screen happens in drawFrame() which calls
    // drawBackground() and drawText() with the prepared vertices
}
