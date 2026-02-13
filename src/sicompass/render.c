#include "view.h"
#include "checkmark.h"
#include <provider_tags.h>
#include <string.h>

// AccessKit node IDs
const accesskit_node_id ROOT_ID = 0;
const accesskit_node_id ELEMENT_ID = 1;
const accesskit_node_id ANNOUNCEMENT_ID = 2;

const Sint32 SET_FOCUS_MSG = 0;

accesskit_node *buildElement(const FfonElement *ffon) {
    accesskit_node *node = accesskit_node_new(ACCESSKIT_ROLE_LIST_ITEM);
    const char *label = (ffon->type == FFON_OBJECT) ?
        ffon->data.object->key : ffon->data.string;
    accesskit_node_set_label(node, label);
    accesskit_node_add_action(node, ACCESSKIT_ACTION_FOCUS);
    return node;
}

accesskit_node *buildAnnouncement(const char *text) {
    accesskit_node *node = accesskit_node_new(ACCESSKIT_ROLE_LIST_ITEM);
    accesskit_node_set_value(node, text);
    accesskit_node_set_live(node, ACCESSKIT_LIVE_POLITE);
    return node;
}

accesskit_node *windowStateBuildRoot(const struct windowState *state) {
    accesskit_node *node = accesskit_node_new(ACCESSKIT_ROLE_WINDOW);
    accesskit_node_push_child(node, ELEMENT_ID);
    if (state->announcement != NULL) {
        accesskit_node_push_child(node, ANNOUNCEMENT_ID);
    }
    accesskit_node_set_label(node, WINDOW_TITLE);
    return node;
}

// Callback for AccessKit activation - returns initial tree
static struct accesskit_tree_update* accesskitActivationHandler(void *userdata) {
    const struct windowState *state = (const struct windowState *)userdata;
    AppRenderer *appRenderer = state->appRenderer;

    windowStateLock((struct windowState *)state);

    accesskit_node *root = windowStateBuildRoot(state);
    accesskit_node *element = buildElement(appRenderer->ffon[0]);
    accesskit_tree_update *result = accesskit_tree_update_with_capacity_and_focus(
        (state->announcement != NULL) ? 4 : 3, state->focus);

    accesskit_tree *tree = accesskit_tree_new(ROOT_ID);
    accesskit_tree_update_set_tree(result, tree);
    accesskit_tree_update_push_node(result, ROOT_ID, root);
    accesskit_tree_update_push_node(result, ELEMENT_ID, element);
    if (state->announcement != NULL) {
        accesskit_node *announcement = buildAnnouncement(state->announcement);
        accesskit_tree_update_push_node(result, ANNOUNCEMENT_ID, announcement);
    }

    windowStateUnlock((struct windowState *)state);

    return result;
}

// Callback for AccessKit action requests - routes to SDL event loop
static void accesskitActionHandler(accesskit_action_request *request, void *userdata) {
    struct actionHandlerState *state = (struct actionHandlerState *)userdata;

    // Push accessibility action as SDL user event
    SDL_Event event;
    SDL_zero(event);
    event.type = state->eventType;
    event.user.windowID = state->windowId;
    event.user.data1 = (void *)((uintptr_t)(request->target_node));
    if (request->action == ACCESSKIT_ACTION_FOCUS) {
        event.user.code = SET_FOCUS_MSG;
        SDL_PushEvent(&event);
    }
    accesskit_action_request_free(request);
}

// Callback for AccessKit deactivation
static void accesskitDeactivationHandler(void *userdata) {
    // Called when assistive technology disconnects
    // There's nothing in the state that depends on whether the adapter is active
}

void accesskitInit(SiCompassApplication *app) {
    app->appRenderer->accesskitRootId = ROOT_ID;
    app->appRenderer->accesskitElementId = ELEMENT_ID;

    // Initialize action handler state
    app->appRenderer->actionHandler.eventType = app->userEvent;
    app->appRenderer->actionHandler.windowId = app->windowId;

    // Create cross-platform SDL adapter
    accesskit_sdl_adapter_init(
        &app->appRenderer->accesskitAdapter,
        app->window,
        accesskitActivationHandler,
        &app->appRenderer->state,           // userdata for activation handler
        accesskitActionHandler,
        &app->appRenderer->actionHandler,  // userdata for action handler
        accesskitDeactivationHandler,
        &app->appRenderer->state            // userdata for deactivation handler
    );

    // Show window after adapter initialization (per AccessKit example)
    SDL_ShowWindow(app->window);
}

void accesskitDestroy(AppRenderer *appRenderer) {
    windowStateDestroy(&appRenderer->state);
    accesskit_sdl_adapter_destroy(&appRenderer->accesskitAdapter);
}

// Window state functions for thread-safe accessibility
void windowStateInit(struct windowState *state, accesskit_node_id initialFocus, AppRenderer *appRenderer) {
    state->focus = initialFocus;
    state->announcement = NULL;
    state->mutex = SDL_CreateMutex();  // SDL3: Returns SDL_Mutex*
    state->appRenderer = appRenderer;
}

void windowStateDestroy(struct windowState *state) {
    if (state->mutex) {
        SDL_DestroyMutex(state->mutex);  // SDL3: Takes SDL_Mutex*
        state->mutex = NULL;
    }
}

void windowStateLock(struct windowState *state) {
    SDL_LockMutex(state->mutex);  // SDL3: Takes SDL_Mutex*
}

void windowStateUnlock(struct windowState *state) {
    SDL_UnlockMutex(state->mutex);  // SDL3: Takes SDL_Mutex*
}

// Factory function for tree updates when speaking
static struct accesskit_tree_update* accesskitSpeakUpdateOnFocus(void *userdata) {
    struct windowState *state = userdata;
    accesskit_tree_update *update = accesskit_tree_update_with_focus(state->focus);
    return update;
}

void accesskitSpeak(AppRenderer *appRenderer, const char *text) {
    if (!text) {
        return;
    }

    windowStateLock(&appRenderer->state);

    accesskit_sdl_adapter_update_if_active(
        &appRenderer->accesskitAdapter,
        accesskitSpeakUpdateOnFocus,
        &appRenderer->state
    );

    windowStateUnlock(&appRenderer->state);
}

void accesskitUpdateWindowFocus(AppRenderer *appRenderer, bool isFocused) {
    accesskit_sdl_adapter_update_window_focus_state(
        &appRenderer->accesskitAdapter, isFocused);
}

void accesskitSpeakCurrentElement(AppRenderer *appRenderer) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
    if (!arr || count == 0) return;

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    FfonElement *elem = arr[idx];
    const char *announcement = (elem->type == FFON_OBJECT) ?
        elem->data.object->key : elem->data.string;

    // appRenderer->state.focus = focus;
    appRenderer->state.announcement = announcement;
    accesskitSpeak(appRenderer, announcement);
}

void accesskitSpeakModeChange(AppRenderer *appRenderer, const char *context) {
    char announcement[512];
    const char *modeName = coordinateToString(appRenderer->currentCoordinate);

    if (context && context[0] != '\0') {
        snprintf(announcement, sizeof(announcement), "%s - %s", modeName, context);
    } else {
        snprintf(announcement, sizeof(announcement), "%s", modeName);
    }

    // appRenderer->state.focus = focus;
    appRenderer->state.announcement = announcement;
    accesskitSpeak(appRenderer, announcement);
}

typedef enum { RADIO_NONE, RADIO_UNCHECKED, RADIO_CHECKED } RadioType;

static RadioType getRadioType(const char *label) {
    if (!label) return RADIO_NONE;
    if (strncmp(label, "-rc ", 4) == 0) return RADIO_CHECKED;
    if (strncmp(label, "-r ", 3) == 0) return RADIO_UNCHECKED;
    return RADIO_NONE;
}

static float renderRadioIndicator(SiCompassApplication *app,
                                  RadioType radioType, int itemX, int itemYPos) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    float lineH = getLineHeight(app, scale, TEXT_PADDING);
    float circleSize = lineH * 0.8f;
    float circleX = (float)itemX;
    float lineTop = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;
    float circleY = lineTop + (lineH - circleSize) / 2.0f;

    // Outer circle (always rendered)
    prepareRectangle(app, circleX, circleY, circleSize, circleSize,
                     COLOR_LIGHT_GREEN, circleSize / 2.0f);

    // Inner circle: green for checked, black (clearvalue) for unchecked
    float innerSize = circleSize * 0.55f;
    float innerOffset = (circleSize - innerSize) / 2.0f;
    uint32_t innerColor = (radioType == RADIO_CHECKED) ? COLOR_DARK_GREEN : 0x000000FF;
    prepareRectangle(app, circleX + innerOffset, circleY + innerOffset,
                     innerSize, innerSize, innerColor, innerSize / 2.0f);

    // Return circle width + one character gap
    float charWidth = getWidthEM(app, scale);
    return circleSize + charWidth;
}

typedef enum { CHECKBOX_NONE, CHECKBOX_UNCHECKED, CHECKBOX_CHECKED } CheckboxType;

static CheckboxType getCheckboxType(const char *label) {
    if (!label) return CHECKBOX_NONE;
    if (strncmp(label, "-cc ", 4) == 0) return CHECKBOX_CHECKED;
    if (strncmp(label, "-c ", 3) == 0) return CHECKBOX_UNCHECKED;
    return CHECKBOX_NONE;
}

static float renderCheckboxIndicator(SiCompassApplication *app,
                                     CheckboxType checkboxType, int itemX, int itemYPos) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    float lineH = getLineHeight(app, scale, TEXT_PADDING);
    float boxSize = lineH * 0.8f;
    float boxX = (float)itemX;
    float lineTop = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;
    float boxY = lineTop + (lineH - boxSize) / 2.0f;

    if (checkboxType == CHECKBOX_CHECKED) {
        // Checked: dark green square with text-colored checkmark
        prepareRectangle(app, boxX, boxY, boxSize, boxSize,
                         COLOR_DARK_GREEN, 0.0f);
        float checkPadding = boxSize * 0.02f;
        float checkSize = boxSize - checkPadding * 2.0f;
        prepareCheckmark(app, boxX + checkPadding, boxY + checkPadding,
                         checkSize, COLOR_TEXT);
    } else {
        // Unchecked: text-colored border with black center
        prepareRectangle(app, boxX, boxY, boxSize, boxSize,
                         COLOR_TEXT, 0.0f);
        float border = boxSize * 0.07f;
        float innerSize = boxSize - border * 2.0f;
        prepareRectangle(app, boxX + border, boxY + border,
                         innerSize, innerSize, 0x000000FF, 0.0f);
    }

    // Return box width + one character gap
    float charWidth = getWidthEM(app, scale);
    return boxSize + charWidth;
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
    int clipY = app->appRenderer->renderClipTopY;
    if (highlight && lineCount > 0) {
        float overallMinX = INFINITY;
        float overallMinY = INFINITY;
        float overallMaxX = -INFINITY;
        float overallMaxY = -INFINITY;

        for (int i = 0; i < lineCount; i++) {
            int currentY = y + i * lineHeight;
            if (clipY > 0 && currentY < clipY) continue;

            char lineText[MAX_LINE_LENGTH];
            strncpy(lineText, lines[i].start, lines[i].len);
            lineText[lines[i].len] = '\0';

            float minX, minY, maxX, maxY;
            calculateTextBounds(app, lineText, (float)x, (float)currentY, scale,
                              &minX, &minY, &maxX, &maxY);

            if (minX < overallMinX) overallMinX = minX;
            if (minY < overallMinY) overallMinY = minY;
            if (maxX > overallMaxX) overallMaxX = maxX;
            if (maxY > overallMaxY) overallMaxY = maxY;
        }

        if (overallMinX != INFINITY) {
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
    }

    // Third pass: render all text lines
    int currentY = y;
    for (int i = 0; i < lineCount; i++) {
        char lineText[MAX_LINE_LENGTH];
        strncpy(lineText, lines[i].start, lines[i].len);
        lineText[lines[i].len] = '\0';

        if (lines[i].len > 0) {
            if (clipY <= 0 || currentY >= clipY) {
                prepareTextForRendering(app, lineText, (float)x, (float)currentY, scale, color);
            }
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
    bool hasRadioSummary = false;
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
                char *strippedParent = providerTagStripDisplay(parentText);
                renderText(app, strippedParent ? strippedParent : parentText, 50, yPos, COLOR_TEXT, false);
                free(strippedParent);
                yPos += lineHeight;

                // Render checked radio summary if parent is a radio group
                if (parentElem->type == FFON_OBJECT &&
                    providerTagHasRadio(parentElem->data.object->key)) {
                    FfonObject *radioObj = parentElem->data.object;
                    for (int i = 0; i < radioObj->count; i++) {
                        FfonElement *child = radioObj->elements[i];
                        if (child->type == FFON_STRING &&
                            providerTagHasChecked(child->data.string)) {
                            char *checkedText = providerTagExtractCheckedContent(child->data.string);
                            if (checkedText) {
                                int summaryX = 50 + indent;
                                float circleWidth = renderRadioIndicator(app, RADIO_CHECKED, summaryX, yPos);
                                summaryX += (int)circleWidth;
                                renderText(app, checkedText, summaryX, yPos, COLOR_TEXT, false);
                                free(checkedText);
                                yPos += lineHeight;
                                hasRadioSummary = true;
                            }
                            break;
                        }
                    }
                }
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

    // Calculate visible item range to keep listIndex in view
    int headerLines = (app->appRenderer->currentId.depth > 1) ? 3 : 2;  // parent + gap = 3, or just header = 2
    if (hasRadioSummary) headerLines++;
    int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
    int visibleItems = availableHeight / lineHeight;
    if (visibleItems < 1) visibleItems = 1;

    int startIndex = app->appRenderer->scrollOffset;
    int scrolloff = 1;

    // Keep margin above: selected stays at position >= scrolloff, unless at first item
    if (app->appRenderer->listIndex > 0 &&
        app->appRenderer->listIndex < startIndex + scrolloff) {
        startIndex = app->appRenderer->listIndex - scrolloff;
    }
    if (app->appRenderer->listIndex < startIndex) {
        startIndex = app->appRenderer->listIndex;
    }

    // Keep margin below: selected stays at position <= visibleItems-1-scrolloff, unless at last item
    if (app->appRenderer->listIndex < count - 1 &&
        app->appRenderer->listIndex >= startIndex + visibleItems - scrolloff) {
        startIndex = app->appRenderer->listIndex - visibleItems + 1 + scrolloff;
    }
    if (app->appRenderer->listIndex >= startIndex + visibleItems) {
        startIndex = app->appRenderer->listIndex - visibleItems + 1;
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    int endIndex = startIndex + visibleItems;
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Render radio indicator before text if this is a radio item
        RadioType radioType = getRadioType(list[i].label);
        if (radioType != RADIO_NONE) {
            float circleWidth = renderRadioIndicator(app, radioType, itemX, itemYPos);
            itemX += (int)circleWidth;
        }

        // Render checkbox indicator before text if this is a checkbox item
        CheckboxType checkboxType = getCheckboxType(list[i].label);
        if (checkboxType != CHECKBOX_NONE) {
            float boxWidth = renderCheckboxIndicator(app, checkboxType, itemX, itemYPos);
            itemX += (int)boxWidth;
        }

        // Determine what text to display
        const char *displayText = list[i].label;

        // In insert mode, show inputBuffer for selected item
        if (isSelected && inInsertMode) {
            displayText = app->appRenderer->inputBuffer;

            // Store position for caret rendering
            app->appRenderer->currentElementX = itemX;
            app->appRenderer->currentElementY = itemYPos;
        }

        // Render text (may be multiple lines)
        int textLines = renderText(app, displayText, itemX, itemYPos, COLOR_TEXT, isSelected);

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

    // Render checked radio summary if inside a radio group
    bool hasRadioSummary = false;
    if (app->appRenderer->currentId.depth >= 2) {
        IdArray parentId;
        idArrayCopy(&parentId, &app->appRenderer->currentId);
        idArrayPop(&parentId);

        int parentCount;
        FfonElement **parentArr = getFfonAtId(app->appRenderer->ffon, app->appRenderer->ffonCount, &parentId, &parentCount);
        if (parentArr && parentCount > 0) {
            int parentIdx = parentId.ids[parentId.depth - 1];
            if (parentIdx >= 0 && parentIdx < parentCount) {
                FfonElement *parentElem = parentArr[parentIdx];
                if (parentElem->type == FFON_OBJECT &&
                    providerTagHasRadio(parentElem->data.object->key)) {
                    FfonObject *radioObj = parentElem->data.object;
                    for (int i = 0; i < radioObj->count; i++) {
                        FfonElement *child = radioObj->elements[i];
                        if (child->type == FFON_STRING &&
                            providerTagHasChecked(child->data.string)) {
                            char *checkedText = providerTagExtractCheckedContent(child->data.string);
                            if (checkedText) {
                                int summaryX = 50 + indent;
                                float circleWidth = renderRadioIndicator(app, RADIO_CHECKED, summaryX, yPos);
                                summaryX += (int)circleWidth;
                                renderText(app, checkedText, summaryX, yPos, COLOR_TEXT, false);
                                free(checkedText);
                                yPos += lineHeight;
                                hasRadioSummary = true;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    ListItem *list = app->appRenderer->filteredListCount > 0 ?
                     app->appRenderer->filteredListCurrentLayer : app->appRenderer->totalListCurrentLayer;
    int count = app->appRenderer->filteredListCount > 0 ?
                app->appRenderer->filteredListCount : app->appRenderer->totalListCount;

    // Calculate visible item range to keep listIndex in view
    int headerLines = 3;  // header line + search input line + gap
    if (hasRadioSummary) headerLines++;
    int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
    int visibleItems = availableHeight / lineHeight;
    if (visibleItems < 1) visibleItems = 1;

    int startIndex = app->appRenderer->scrollOffset;
    int scrolloff = 1;

    // Keep margin above: selected stays at position >= scrolloff, unless at first item
    if (app->appRenderer->listIndex > 0 &&
        app->appRenderer->listIndex < startIndex + scrolloff) {
        startIndex = app->appRenderer->listIndex - scrolloff;
    }
    if (app->appRenderer->listIndex < startIndex) {
        startIndex = app->appRenderer->listIndex;
    }

    // Keep margin below: selected stays at position <= visibleItems-1-scrolloff, unless at last item
    if (app->appRenderer->listIndex < count - 1 &&
        app->appRenderer->listIndex >= startIndex + visibleItems - scrolloff) {
        startIndex = app->appRenderer->listIndex - visibleItems + 1 + scrolloff;
    }
    if (app->appRenderer->listIndex >= startIndex + visibleItems) {
        startIndex = app->appRenderer->listIndex - visibleItems + 1;
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    int endIndex = startIndex + visibleItems;
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Render radio indicator before text if this is a radio item
        RadioType radioType = getRadioType(list[i].label);
        if (radioType != RADIO_NONE) {
            float circleWidth = renderRadioIndicator(app, radioType, itemX, itemYPos);
            itemX += (int)circleWidth;
        }

        // Render checkbox indicator before text if this is a checkbox item
        CheckboxType checkboxType = getCheckboxType(list[i].label);
        if (checkboxType != CHECKBOX_NONE) {
            float boxWidth = renderCheckboxIndicator(app, checkboxType, itemX, itemYPos);
            itemX += (int)boxWidth;
        }

        // Render text (may be multiple lines)
        int textLines = renderText(app, list[i].label, itemX, itemYPos, COLOR_TEXT, isSelected);

        yPos += lineHeight * textLines;
    }
}

void renderScroll(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);
    int yPos = lineHeight * 2 - app->appRenderer->textScrollOffset * lineHeight;

    // Clip content to its initial top position (below header + gap)
    app->appRenderer->renderClipTopY = lineHeight * 2;

    int count;
    FfonElement **arr = getFfonAtId(app->appRenderer->ffon, app->appRenderer->ffonCount,
                                     &app->appRenderer->currentId, &count);
    if (arr && count > 0) {
        int idx = app->appRenderer->currentId.ids[app->appRenderer->currentId.depth - 1];
        if (idx >= 0 && idx < count) {
            FfonElement *elem = arr[idx];
            const char *text = (elem->type == FFON_OBJECT) ?
                elem->data.object->key : elem->data.string;
            char *stripped = providerTagStripDisplay(text);
            int lines = renderText(app, stripped ? stripped : text, 50, yPos, COLOR_TEXT, true);
            app->appRenderer->textScrollLineCount = lines;
            free(stripped);
        }
    }

    app->appRenderer->renderClipTopY = 0;
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
    snprintf(header, sizeof(header), "%s, layer: %d list: %d/%d",
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
        renderText(app, app->appRenderer->errorMessage, maxX + 20, (float)headerHeight, COLOR_RED, false);
    }

    // Render appropriate panel
    if (app->appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        renderScroll(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
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
    }

    // Render selection highlight in all text input modes
    if (hasSelection(app->appRenderer)) {
        int selStart, selEnd;
        getSelectionRange(app->appRenderer, &selStart, &selEnd);

        int baseX, baseY;
        if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
            app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
            // Search/command modes: account for "search: " prefix
            int searchTextYPos = lineHeight * 2;
            float pfxMinX, pfxMinY, pfxMaxX, pfxMaxY;
            calculateTextBounds(app, "search: ", 50.0f, (float)searchTextYPos, scale,
                                &pfxMinX, &pfxMinY, &pfxMaxX, &pfxMaxY);
            baseX = 50 + (int)(pfxMaxX - pfxMinX);
            baseY = searchTextYPos;
        } else {
            // Insert modes: use stored element position
            baseX = app->appRenderer->currentElementX;
            baseY = app->appRenderer->currentElementY;
        }

        // Get proper Y from text bounds
        float tMinX, tMinY, tMaxX, tMaxY;
        calculateTextBounds(app, " ", (float)baseX, (float)baseY, scale,
                            &tMinX, &tMinY, &tMaxX, &tMaxY);
        float selY = tMinY;

        // Calculate X start of selection
        float selXStart = (float)baseX;
        if (selStart > 0) {
            char tempStr[MAX_LINE_LENGTH];
            int copyLen = selStart < MAX_LINE_LENGTH - 1 ? selStart : MAX_LINE_LENGTH - 1;
            strncpy(tempStr, app->appRenderer->inputBuffer, copyLen);
            tempStr[copyLen] = '\0';
            float sMinX, sMinY, sMaxX, sMaxY;
            calculateTextBounds(app, tempStr, (float)baseX, (float)baseY, scale,
                                &sMinX, &sMinY, &sMaxX, &sMaxY);
            selXStart = sMaxX;
        }

        // Calculate X end of selection
        char tempStr2[MAX_LINE_LENGTH];
        int copyLen2 = selEnd < MAX_LINE_LENGTH - 1 ? selEnd : MAX_LINE_LENGTH - 1;
        strncpy(tempStr2, app->appRenderer->inputBuffer, copyLen2);
        tempStr2[copyLen2] = '\0';
        float eMinX, eMinY, eMaxX, eMaxY;
        calculateTextBounds(app, tempStr2, (float)baseX, (float)baseY, scale,
                            &eMinX, &eMinY, &eMaxX, &eMaxY);
        float selXEnd = eMaxX;

        // Render selection rectangle
        float selWidth = selXEnd - selXStart;
        float selHeight = getLineHeight(app, scale, TEXT_PADDING) - (2.0f * TEXT_PADDING);
        prepareRectangle(app, selXStart, selY, selWidth, selHeight,
                         COLOR_SELECTION, 0.0f);
    }

    if (app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
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
