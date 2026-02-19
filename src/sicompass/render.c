#include "view.h"
#include "checkmark.h"
#include "image.h"
#include "unicode_search.h"
#include <provider_tags.h>
#include <string.h>
#include <math.h>

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
                     app->appRenderer->palette->text, circleSize / 2.0f);

    // Inner circle: light green for checked, white for unchecked
    float innerSize = circleSize * 0.55f;
    float innerOffset = (circleSize - innerSize) / 2.0f;
    uint32_t innerColor = (radioType == RADIO_CHECKED)
        ? app->appRenderer->palette->selected
        : app->appRenderer->palette->background;
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
        // Checked: highlight square with text-colored checkmark
        prepareRectangle(app, boxX, boxY, boxSize, boxSize,
                         app->appRenderer->palette->selected, 0.0f);
        float checkPadding = boxSize * 0.02f;
        float checkSize = boxSize - checkPadding * 2.0f;
        prepareCheckmark(app, boxX + checkPadding, boxY + checkPadding,
                         checkSize, app->appRenderer->palette->text);
    } else {
        // Unchecked: text-colored border with bg-colored center
        prepareRectangle(app, boxX, boxY, boxSize, boxSize,
                         app->appRenderer->palette->text, 0.0f);
        float border = boxSize * 0.07f;
        float innerSize = boxSize - border * 2.0f;
        prepareRectangle(app, boxX + border, boxY + border,
                         innerSize, innerSize, app->appRenderer->palette->background, 0.0f);
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

            prepareRectangle(app, overallMinX, overallMinY, width, height, app->appRenderer->palette->selected, cornerRadius);
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
//         uint32_t color = app->appRenderer->palette->text;
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

//         uint32_t color = app->appRenderer->palette->text;
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
//         renderText(app, displayText, 50, yPos, app->appRenderer->palette->text, true);
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

static int getItemLineCount(const char *label, SiCompassApplication *app,
                            float charWidth, int lineHeight, int headerLines) {
    if (label && strncmp(label, "-p ", 3) == 0) {
        const char *imagePath = label + 3;
        if (loadImageTexture(app, imagePath)) {
            ImageRenderer *ir = app->imageRenderer;
            float imgW = (float)ir->textureWidth;
            float imgH = (float)ir->textureHeight;

            float maxW = charWidth * 120.0f;
            float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * headerLines);

            float displayScale = 1.0f;
            if (imgW > maxW) displayScale = maxW / imgW;
            if (imgH * displayScale > maxH) displayScale = maxH / imgH;

            float displayH = imgH * displayScale;
            int imageLines = (int)ceilf(displayH / (float)lineHeight);
            return imageLines > 1 ? imageLines : 1;
        }
    }
    return 1;
}

void renderInteraction(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);
    float charWidth = getWidthEM(app, scale);

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
                renderText(app, strippedParent ? strippedParent : parentText, 50, yPos, app->appRenderer->palette->text, false);
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
                                renderText(app, checkedText, summaryX, yPos, app->appRenderer->palette->text, false);
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

    // Calculate visible line range to keep listIndex in view
    int headerLines = (app->appRenderer->currentId.depth > 1) ? 3 : 2;  // parent + gap = 3, or just header = 2
    if (hasRadioSummary) headerLines++;
    int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
    int availableLines = availableHeight / lineHeight;
    if (availableLines < 1) availableLines = 1;

    int startIndex = app->appRenderer->scrollOffset;
    int listIndex = app->appRenderer->listIndex;
    if (listIndex >= count) {
        listIndex = count - 1;
        app->appRenderer->listIndex = listIndex;
    }

    // Ensure startIndex <= listIndex
    if (listIndex < startIndex) {
        startIndex = listIndex;
    }

    // Accumulate lines from startIndex to listIndex (inclusive) to check if it fits
    int linesToSelected = 0;
    for (int i = startIndex; i <= listIndex; i++) {
        linesToSelected += getItemLineCount(list[i].label, app, charWidth, lineHeight, headerLines);
    }

    // If selected item extends past viewport, scroll forward
    while (linesToSelected > availableLines && startIndex < listIndex) {
        linesToSelected -= getItemLineCount(list[startIndex].label, app, charWidth, lineHeight, headerLines);
        startIndex++;
    }

    // Try to show 1 item above selected if possible (scrolloff)
    if (startIndex > 0 && startIndex == listIndex) {
        int prevLines = getItemLineCount(list[startIndex - 1].label, app, charWidth, lineHeight, headerLines);
        if (linesToSelected + prevLines <= availableLines) {
            startIndex--;
        }
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    // Calculate endIndex: include items whose start position is within viewport
    int totalLines = 0;
    int endIndex = startIndex;
    while (endIndex < count) {
        if (totalLines >= availableLines) break;
        totalLines += getItemLineCount(list[endIndex].label, app, charWidth, lineHeight, headerLines);
        endIndex++;
    }
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Check if this is an image item
        if (list[i].label && strncmp(list[i].label, "-p ", 3) == 0) {
            const char *imagePath = list[i].label + 3;

            if (loadImageTexture(app, imagePath)) {
                ImageRenderer *ir = app->imageRenderer;
                float imgW = (float)ir->textureWidth;
                float imgH = (float)ir->textureHeight;

                // Calculate max display dimensions
                float maxW = charWidth * 120.0f;
                float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * headerLines);

                // Scale to fit within constraints, maintaining aspect ratio
                float displayScale = 1.0f;
                if (imgW > maxW) {
                    displayScale = maxW / imgW;
                }
                if (imgH * displayScale > maxH) {
                    displayScale = maxH / imgH;
                }

                float displayW = imgW * displayScale;
                float displayH = imgH * displayScale;

                // Position image at current item location
                float imgX = (float)itemX;
                float imgY = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

                // Render dark green border around image when selected
                if (isSelected) {
                    float border = 2.0f;
                    prepareRectangle(app, imgX - border, imgY - border,
                                     displayW + border * 2.0f, displayH + border * 2.0f,
                                     app->appRenderer->palette->selected, 0.0f);
                }

                prepareImage(app, imgX, imgY, displayW, displayH);

                int imageLines = (int)ceilf(displayH / (float)lineHeight);
                if (imageLines < 1) imageLines = 1;
                yPos += lineHeight * imageLines;
            } else {
                // Failed to load image, show path as text
                int textLines = renderText(app, imagePath, itemX, itemYPos, app->appRenderer->palette->text, isSelected);
                yPos += lineHeight * textLines;
            }
            continue;
        }

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
        int textLines = renderText(app, displayText, itemX, itemYPos, app->appRenderer->palette->text, isSelected);

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
    int linesRendered = renderText(app, searchText, 50, yPos, app->appRenderer->palette->text, false);
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
                                renderText(app, checkedText, summaryX, yPos, app->appRenderer->palette->text, false);
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

    if (!list || count == 0) return;

    // Calculate visible line range to keep listIndex in view
    int headerLines = 3;  // header line + search input line + gap
    if (hasRadioSummary) headerLines++;
    int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
    float charWidth = getWidthEM(app, scale);
    int availableLines = availableHeight / lineHeight;
    if (availableLines < 1) availableLines = 1;

    int startIndex = app->appRenderer->scrollOffset;
    int listIndex = app->appRenderer->listIndex;
    if (listIndex >= count) {
        listIndex = count - 1;
        app->appRenderer->listIndex = listIndex;
    }

    // Ensure startIndex <= listIndex
    if (listIndex < startIndex) {
        startIndex = listIndex;
    }

    // Accumulate lines from startIndex to listIndex (inclusive) to check if it fits
    int linesToSelected = 0;
    for (int i = startIndex; i <= listIndex; i++) {
        linesToSelected += getItemLineCount(list[i].label, app, charWidth, lineHeight, headerLines);
    }

    // If selected item extends past viewport, scroll forward
    while (linesToSelected > availableLines && startIndex < listIndex) {
        linesToSelected -= getItemLineCount(list[startIndex].label, app, charWidth, lineHeight, headerLines);
        startIndex++;
    }

    // Try to show 1 item above selected if possible (scrolloff)
    if (startIndex > 0 && startIndex == listIndex) {
        int prevLines = getItemLineCount(list[startIndex - 1].label, app, charWidth, lineHeight, headerLines);
        if (linesToSelected + prevLines <= availableLines) {
            startIndex--;
        }
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    // Calculate endIndex: include items whose start position is within viewport
    int totalLines = 0;
    int endIndex = startIndex;
    while (endIndex < count) {
        if (totalLines >= availableLines) break;
        totalLines += getItemLineCount(list[endIndex].label, app, charWidth, lineHeight, headerLines);
        endIndex++;
    }
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Check if this is an image item
        if (list[i].label && strncmp(list[i].label, "-p ", 3) == 0) {
            const char *imagePath = list[i].label + 3;

            if (loadImageTexture(app, imagePath)) {
                ImageRenderer *ir = app->imageRenderer;
                float imgW = (float)ir->textureWidth;
                float imgH = (float)ir->textureHeight;

                float maxW = charWidth * 120.0f;
                float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * headerLines);

                float displayScale = 1.0f;
                if (imgW > maxW) displayScale = maxW / imgW;
                if (imgH * displayScale > maxH) displayScale = maxH / imgH;

                float displayW = imgW * displayScale;
                float displayH = imgH * displayScale;

                float imgX = (float)itemX;
                float imgY = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

                if (isSelected) {
                    float border = 2.0f;
                    prepareRectangle(app, imgX - border, imgY - border,
                                     displayW + border * 2.0f, displayH + border * 2.0f,
                                     app->appRenderer->palette->selected, 0.0f);
                }

                prepareImage(app, imgX, imgY, displayW, displayH);

                int imageLines = (int)ceilf(displayH / (float)lineHeight);
                if (imageLines < 1) imageLines = 1;
                yPos += lineHeight * imageLines;
            } else {
                int textLines = renderText(app, imagePath, itemX, itemYPos, app->appRenderer->palette->text, isSelected);
                yPos += lineHeight * textLines;
            }
            continue;
        }

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
        int textLines = renderText(app, list[i].label, itemX, itemYPos, app->appRenderer->palette->text, isSelected);

        yPos += lineHeight * textLines;
    }
}

void renderExtendedSearch(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // Calculate indent as the actual width of 4 spaces using text bounds
    float minX, minY, maxX, maxY;
    calculateTextBounds(app, "    ", 0.0f, 0.0f, scale, &minX, &minY, &maxX, &maxY);
    int indent = (int)(maxX - minX);

    int yPos = lineHeight * 2;

    // Render search input
    char searchText[MAX_LINE_LENGTH];
    snprintf(searchText, sizeof(searchText), "ext search: %s", app->appRenderer->inputBuffer);
    int linesRendered = renderText(app, searchText, 50, yPos, app->appRenderer->palette->text, false);
    yPos += lineHeight * linesRendered;

    ListItem *list = app->appRenderer->filteredListCount > 0 ?
                     app->appRenderer->filteredListCurrentLayer : app->appRenderer->totalListCurrentLayer;
    int count = app->appRenderer->filteredListCount > 0 ?
                app->appRenderer->filteredListCount : app->appRenderer->totalListCount;

    if (!list || count == 0) return;

    // Calculate visible line range to keep listIndex in view
    int headerLines = 3;  // header line + search input line + gap
    int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
    float charWidth = getWidthEM(app, scale);
    int availableLines = availableHeight / lineHeight;
    if (availableLines < 1) availableLines = 1;

    int startIndex = app->appRenderer->scrollOffset;
    int listIndex = app->appRenderer->listIndex;
    if (listIndex >= count) {
        listIndex = count - 1;
        app->appRenderer->listIndex = listIndex;
    }

    // Ensure startIndex <= listIndex
    if (listIndex < startIndex) {
        startIndex = listIndex;
    }

    // Accumulate lines from startIndex to listIndex (inclusive) to check if it fits
    int linesToSelected = 0;
    for (int i = startIndex; i <= listIndex; i++) {
        linesToSelected += getItemLineCount(list[i].label, app, charWidth, lineHeight, headerLines);
    }

    // If selected item extends past viewport, scroll forward
    while (linesToSelected > availableLines && startIndex < listIndex) {
        linesToSelected -= getItemLineCount(list[startIndex].label, app, charWidth, lineHeight, headerLines);
        startIndex++;
    }

    // Try to show 1 item above selected if possible (scrolloff)
    if (startIndex > 0 && startIndex == listIndex) {
        int prevLines = getItemLineCount(list[startIndex - 1].label, app, charWidth, lineHeight, headerLines);
        if (linesToSelected + prevLines <= availableLines) {
            startIndex--;
        }
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    // Calculate endIndex: include items whose start position is within viewport
    int totalLines = 0;
    int endIndex = startIndex;
    while (endIndex < count) {
        if (totalLines >= availableLines) break;
        totalLines += getItemLineCount(list[endIndex].label, app, charWidth, lineHeight, headerLines);
        endIndex++;
    }
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Render breadcrumb prefix if present
        if (list[i].data && ((char *)list[i].data)[0] != '\0') {
            float bMinX, bMinY, bMaxX, bMaxY;
            calculateTextBounds(app, list[i].data, (float)itemX, (float)itemYPos, scale,
                                &bMinX, &bMinY, &bMaxX, &bMaxY);
            renderText(app, list[i].data, itemX, itemYPos, app->appRenderer->palette->extsearch, false);
            itemX += (int)(bMaxX - bMinX);
        }

        // Check if this is an image item
        if (list[i].label && strncmp(list[i].label, "-p ", 3) == 0) {
            const char *imagePath = list[i].label + 3;

            if (loadImageTexture(app, imagePath)) {
                ImageRenderer *ir = app->imageRenderer;
                float imgW = (float)ir->textureWidth;
                float imgH = (float)ir->textureHeight;

                float maxW = charWidth * 120.0f;
                float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * headerLines);

                float displayScale = 1.0f;
                if (imgW > maxW) displayScale = maxW / imgW;
                if (imgH * displayScale > maxH) displayScale = maxH / imgH;

                float displayW = imgW * displayScale;
                float displayH = imgH * displayScale;

                float imgX = (float)itemX;
                float imgY = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

                if (isSelected) {
                    float border = 2.0f;
                    prepareRectangle(app, imgX - border, imgY - border,
                                     displayW + border * 2.0f, displayH + border * 2.0f,
                                     app->appRenderer->palette->selected, 0.0f);
                }

                prepareImage(app, imgX, imgY, displayW, displayH);

                int imageLines = (int)ceilf(displayH / (float)lineHeight);
                if (imageLines < 1) imageLines = 1;
                yPos += lineHeight * imageLines;
            } else {
                int textLines = renderText(app, imagePath, itemX, itemYPos, app->appRenderer->palette->text, isSelected);
                yPos += lineHeight * textLines;
            }
            continue;
        }

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
        int textLines = renderText(app, list[i].label, itemX, itemYPos, app->appRenderer->palette->text, isSelected);

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

            if (providerTagHasImage(text)) {
                char *imagePath = providerTagExtractImageContent(text);
                if (imagePath && loadImageTexture(app, imagePath)) {
                    ImageRenderer *ir = app->imageRenderer;
                    float imgW = (float)ir->textureWidth;
                    float imgH = (float)ir->textureHeight;
                    float charWidth = getWidthEM(app, scale);

                    // Fit to width only, no height constraint for scroll mode
                    float maxW = charWidth * 120.0f;
                    float displayScale = 1.0f;
                    if (imgW > maxW) displayScale = maxW / imgW;

                    float displayW = imgW * displayScale;
                    float displayH = imgH * displayScale;

                    float imgX = 50.0f;
                    float imgY = 1.5f * (float)lineHeight - (float)(app->appRenderer->textScrollOffset * lineHeight);

                    prepareImage(app, imgX, imgY, displayW, displayH);

                    // Clip top edge at header boundary
                    float clipTop = 1.5f * (float)lineHeight;
                    if (imgY < clipTop && ir->drawCallCount > 0) {
                        float uvTop = (clipTop - imgY) / displayH;
                        ImageDrawCall *dc = &ir->drawCalls[ir->drawCallCount - 1];
                        dc->vertices[0].pos[1] = clipTop;
                        dc->vertices[0].texCoord[1] = uvTop;
                        dc->vertices[1].pos[1] = clipTop;
                        dc->vertices[1].texCoord[1] = uvTop;
                        dc->vertices[3].pos[1] = clipTop;
                        dc->vertices[3].texCoord[1] = uvTop;
                    }

                    int imageLines = (int)ceilf(displayH / (float)lineHeight);
                    app->appRenderer->textScrollLineCount = imageLines > 1 ? imageLines : 1;
                } else {
                    int lines = renderText(app, imagePath ? imagePath : text, 50, yPos, app->appRenderer->palette->text, true);
                    app->appRenderer->textScrollLineCount = lines;
                }
                free(imagePath);
            } else {
                char *stripped = providerTagStripDisplay(text);
                int lines = renderText(app, stripped ? stripped : text, 50, yPos, app->appRenderer->palette->text, true);
                app->appRenderer->textScrollLineCount = lines;
                free(stripped);
            }
        }
    }

    app->appRenderer->renderClipTopY = 0;
}

void renderScrollSearch(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);
    float charWidth = getWidthEM(app, scale);
    float maxWidth = charWidth * 120.0f;

    // Get the text to search in
    int count;
    FfonElement **arr = getFfonAtId(app->appRenderer->ffon, app->appRenderer->ffonCount,
                                     &app->appRenderer->currentId, &count);
    const char *rawText = "";
    char *stripped = NULL;
    if (arr && count > 0) {
        int idx = app->appRenderer->currentId.ids[app->appRenderer->currentId.depth - 1];
        if (idx >= 0 && idx < count) {
            FfonElement *elem = arr[idx];
            rawText = (elem->type == FFON_OBJECT) ?
                elem->data.object->key : elem->data.string;
        }
    }

    // Handle image elements: show search bar with 0 matches and render image
    if (providerTagHasImage(rawText)) {
        char searchDisplay[MAX_LINE_LENGTH];
        snprintf(searchDisplay, sizeof(searchDisplay), "search: %s [0 items]",
                 app->appRenderer->inputBuffer);
        renderText(app, searchDisplay, 50, lineHeight * 2, app->appRenderer->palette->text, false);
        app->appRenderer->scrollSearchMatchCount = 0;
        app->appRenderer->scrollSearchCurrentMatch = 0;

        // Render the image below the search bar
        char *imagePath = providerTagExtractImageContent(rawText);
        if (imagePath && loadImageTexture(app, imagePath)) {
            ImageRenderer *ir = app->imageRenderer;
            float imgW = (float)ir->textureWidth;
            float imgH = (float)ir->textureHeight;
            float imgMaxW = charWidth * 120.0f;
            float displayScale = 1.0f;
            if (imgW > imgMaxW) displayScale = imgMaxW / imgW;
            float displayW = imgW * displayScale;
            float displayH = imgH * displayScale;
            float imgX = 50.0f;
            float imgY = (float)(lineHeight * 3) - (float)(app->appRenderer->textScrollOffset * lineHeight);
            prepareImage(app, imgX, imgY, displayW, displayH);
            int imageLines = (int)ceilf(displayH / (float)lineHeight);
            app->appRenderer->textScrollLineCount = imageLines > 1 ? imageLines : 1;
        }
        free(imagePath);
        return;
    }

    stripped = providerTagStripDisplay(rawText);
    const char *text = stripped ? stripped : rawText;

    // Line-wrap the text (same algorithm as renderText first pass)
    typedef struct {
        const char *start;
        size_t len;
        int byteOffset;
    } LineInfo;

    LineInfo lines[1000];
    int lineCount = 0;
    const char *lineStart = text;

    while (*lineStart != '\0' && lineCount < 1000) {
        const char *lineEnd = lineStart;
        const char *lastSpace = NULL;
        const char *lastFit = lineStart;
        int currentY = lineHeight * 3 + lineCount * lineHeight;

        while (*lineEnd != '\0') {
            size_t testLen = lineEnd - lineStart + 1;
            if (testLen >= MAX_LINE_LENGTH) testLen = MAX_LINE_LENGTH - 1;

            char testText[MAX_LINE_LENGTH];
            strncpy(testText, lineStart, testLen);
            testText[testLen] = '\0';

            float minX, minY, maxX, maxY;
            calculateTextBounds(app, testText, 50.0f, (float)currentY, scale,
                              &minX, &minY, &maxX, &maxY);
            float width = maxX - minX;

            if (width > maxWidth) {
                if (lastSpace != NULL && lastSpace > lineStart) {
                    lineEnd = lastSpace;
                } else {
                    lineEnd = lastFit;
                }
                break;
            }

            if (*lineEnd == ' ') lastSpace = lineEnd;
            lineEnd++;
            lastFit = lineEnd;
        }

        size_t lineLen = lineEnd - lineStart;
        if (lineLen == 0 && *lineStart != '\0') {
            lineLen = 1;
            lineEnd = lineStart + 1;
        }
        if (lineLen >= MAX_LINE_LENGTH) lineLen = MAX_LINE_LENGTH - 1;

        lines[lineCount].start = lineStart;
        lines[lineCount].len = lineLen;
        lines[lineCount].byteOffset = (int)(lineStart - text);
        lineCount++;

        lineStart = lineEnd;
        if (*lineStart == ' ') lineStart++;
    }

    app->appRenderer->textScrollLineCount = lineCount;

    // Find all matches
    typedef struct {
        int byteOffset;
        int length;
        int wrappedLine;
        int lineLocalByte;
    } MatchInfo;

    MatchInfo matches[500];
    int matchCount = 0;
    const char *searchTerm = app->appRenderer->inputBuffer;
    int searchLen = app->appRenderer->inputBufferSize;

    if (searchLen > 0) {
        const char *pos = text;
        while (pos && *pos && matchCount < 500) {
            const char *found = utf8_stristr_pos(pos, searchTerm);
            if (!found) break;

            int byteOffset = (int)(found - text);

            // Determine match length in original text (search for needle length bytes)
            // For simple case, use searchLen; case folding may differ but this is close enough
            int matchLen = searchLen;

            matches[matchCount].byteOffset = byteOffset;
            matches[matchCount].length = matchLen;
            matches[matchCount].wrappedLine = 0;
            matches[matchCount].lineLocalByte = 0;

            // Find which wrapped line this match falls on
            for (int line = 0; line < lineCount; line++) {
                int lStart = lines[line].byteOffset;
                int lEnd = lStart + (int)lines[line].len;
                if (byteOffset >= lStart && byteOffset < lEnd) {
                    matches[matchCount].wrappedLine = line;
                    matches[matchCount].lineLocalByte = byteOffset - lStart;
                    break;
                }
            }

            matchCount++;
            // Advance past this match start by one byte to find overlapping/next matches
            pos = found + 1;
        }
    }

    app->appRenderer->scrollSearchMatchCount = matchCount;
    if (app->appRenderer->scrollSearchCurrentMatch >= matchCount) {
        app->appRenderer->scrollSearchCurrentMatch = matchCount > 0 ? matchCount - 1 : 0;
    }

    // Auto-scroll to current match
    if (matchCount > 0) {
        int currentIdx = app->appRenderer->scrollSearchCurrentMatch;
        int matchLine = matches[currentIdx].wrappedLine;

        int headerLines = 3;
        int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
        int visibleLines = availableHeight / lineHeight;
        if (visibleLines < 1) visibleLines = 1;

        if (matchLine < app->appRenderer->textScrollOffset) {
            app->appRenderer->textScrollOffset = matchLine;
        } else if (matchLine >= app->appRenderer->textScrollOffset + visibleLines) {
            app->appRenderer->textScrollOffset = matchLine - visibleLines + 1;
        }

        int maxOffset = lineCount - visibleLines;
        if (maxOffset < 0) maxOffset = 0;
        if (app->appRenderer->textScrollOffset > maxOffset)
            app->appRenderer->textScrollOffset = maxOffset;
        if (app->appRenderer->textScrollOffset < 0)
            app->appRenderer->textScrollOffset = 0;
    }

    // Render search bar
    char searchDisplay[MAX_LINE_LENGTH];
    snprintf(searchDisplay, sizeof(searchDisplay), "search: %s [%d items]",
             app->appRenderer->inputBuffer, matchCount);
    renderText(app, searchDisplay, 50, lineHeight * 2, app->appRenderer->palette->text, false);

    // Render text with highlights
    int textStartY = lineHeight * 3;
    app->appRenderer->renderClipTopY = textStartY;

    for (int i = 0; i < lineCount; i++) {
        int currentY = textStartY + (i - app->appRenderer->textScrollOffset) * lineHeight;

        // Skip lines above viewport
        if (currentY + lineHeight < textStartY) continue;
        // Stop if below viewport
        if (currentY > (int)app->swapChainExtent.height) break;

        // Render match highlight rectangles for this line
        for (int m = 0; m < matchCount; m++) {
            if (matches[m].wrappedLine != i) continue;
            if (currentY < textStartY) continue;

            int localStart = matches[m].lineLocalByte;
            int localEnd = localStart + matches[m].length;
            if (localEnd > (int)lines[i].len) localEnd = (int)lines[i].len;

            // Measure X position of match start
            float matchX = 50.0f;
            if (localStart > 0) {
                char prefix[MAX_LINE_LENGTH];
                strncpy(prefix, lines[i].start, localStart);
                prefix[localStart] = '\0';

                float pMinX, pMinY, pMaxX, pMaxY;
                calculateTextBounds(app, prefix, 50.0f, (float)currentY, scale,
                                    &pMinX, &pMinY, &pMaxX, &pMaxY);
                matchX = pMaxX;
            }

            // Measure match text width
            int matchLen = localEnd - localStart;
            char matchStr[MAX_LINE_LENGTH];
            strncpy(matchStr, lines[i].start + localStart, matchLen);
            matchStr[matchLen] = '\0';

            float mMinX, mMinY, mMaxX, mMaxY;
            calculateTextBounds(app, matchStr, matchX, (float)currentY, scale,
                                &mMinX, &mMinY, &mMaxX, &mMaxY);

            uint32_t highlightColor = (m == app->appRenderer->scrollSearchCurrentMatch)
                ? app->appRenderer->palette->scrollsearch : app->appRenderer->palette->selected;

            float rectY = mMinY - TEXT_PADDING;
            float rectH = getLineHeight(app, scale, TEXT_PADDING);
            float rectW = mMaxX - matchX;
            prepareRectangle(app, matchX, rectY, rectW, rectH, highlightColor, 3.0f);
        }

        // Render the text itself
        char lineText[MAX_LINE_LENGTH];
        strncpy(lineText, lines[i].start, lines[i].len);
        lineText[lines[i].len] = '\0';

        if (lines[i].len > 0 && currentY >= textStartY) {
            prepareTextForRendering(app, lineText, 50.0f, (float)currentY, scale, app->appRenderer->palette->text);
        }
    }

    app->appRenderer->renderClipTopY = 0;
    free(stripped);
}

void updateView(SiCompassApplication *app) {
    // Note: Screen clearing is handled by the Vulkan rendering pipeline in drawFrame()

    // Begin text rendering for this frame
    beginTextRendering(app);

    // Begin rectangle rendering for this frame (resets rectangle count)
    beginRectangleRendering(app);

    // Draw background fill (covers the Vulkan clear color with the palette background)
    prepareRectangle(app, 0.0f, 0.0f,
                     (float)app->swapChainExtent.width,
                     (float)app->swapChainExtent.height,
                     app->appRenderer->palette->background, 0.0f);

    // Begin image rendering for this frame (resets vertex count)
    beginImageRendering(app);

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
    prepareRectangle(app, 0.0f, (float)lineHeight, headerWidth, lineThickness, app->appRenderer->palette->headerseparator, 0.0f);

    renderText(app, header, (float)50, (float)headerHeight, app->appRenderer->palette->text, false);

    // Render error message if any
    if (app->appRenderer->errorMessage[0] != '\0') {
        renderText(app, app->appRenderer->errorMessage, maxX + 20, (float)headerHeight, app->appRenderer->palette->error, false);
    }

    // Render appropriate panel
    if (app->appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        renderScroll(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        renderScrollSearch(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        app->appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        renderSimpleSearch(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        renderExtendedSearch(app);
    } else {
        renderInteraction(app);
    }

    // Render caret for all modes at end of frame
    if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
        app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // Caret in search field
        int searchTextYPos = lineHeight * 2;

        // Calculate X offset for search prefix and get actual text Y position
        const char *searchPrefix = (app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)
            ? "ext search: " : "search: ";
        float minX, minY, maxX, maxY;
        calculateTextBounds(app, searchPrefix, 50.0f, (float)searchTextYPos, scale,
                          &minX, &minY, &maxX, &maxY);
        int searchPrefixWidth = (int)(maxX - minX);

        // Use minY from text bounds for proper vertical alignment
        caretRender(app, app->appRenderer->caretState,
                   app->appRenderer->inputBuffer,
                   50 + searchPrefixWidth, (int)minY,
                   app->appRenderer->cursorPosition,
                   app->appRenderer->palette->text);
    }

    // Render selection highlight in all text input modes
    if (hasSelection(app->appRenderer)) {
        int selStart, selEnd;
        getSelectionRange(app->appRenderer, &selStart, &selEnd);

        int baseX, baseY;
        if (app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
            app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
            app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
            // Search/command modes: account for search prefix
            const char *selPrefix = (app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)
                ? "ext search: " : "search: ";
            int searchTextYPos = lineHeight * 2;
            float pfxMinX, pfxMinY, pfxMaxX, pfxMaxY;
            calculateTextBounds(app, selPrefix, 50.0f, (float)searchTextYPos, scale,
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
                         app->appRenderer->palette->selected, 0.0f);
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
                   app->appRenderer->palette->text);
    }

    // The actual drawing to the screen happens in drawFrame() which calls
    // drawBackground() and drawText() with the prepared vertices
}
