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
    accesskit_node_set_label(node, text);
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
    accesskit_node *element = (appRenderer->ffonCount > 0 && appRenderer->ffon[0])
        ? buildElement(appRenderer->ffon[0])
        : accesskit_node_new(ACCESSKIT_ROLE_LIST_ITEM);
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
    // accesskit_action_request uses 'target' on Windows, 'target_node' on Linux
    #ifdef _WIN32
    event.user.data1 = (void *)((uintptr_t)(request->target));
    #else
    event.user.data1 = (void *)((uintptr_t)(request->target_node));
    #endif
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
    size_t capacity = (state->announcement != NULL) ? 2 : 1;
    accesskit_tree_update *update =
        accesskit_tree_update_with_capacity_and_focus(capacity, state->focus);
    accesskit_node *root = windowStateBuildRoot(state);
    accesskit_tree_update_push_node(update, ROOT_ID, root);
    if (state->announcement != NULL) {
        accesskit_node *ann = buildAnnouncement(state->announcement);
        accesskit_tree_update_push_node(update, ANNOUNCEMENT_ID, ann);
    }
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

    appRenderer->state.announcement = NULL;

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
    const char *modeName = coordinateToString(appRenderer->currentCoordinate);

    if (context && context[0] != '\0') {
        snprintf(appRenderer->state.announcementBuf,
                 sizeof(appRenderer->state.announcementBuf),
                 "%s - %s", modeName, context);
    } else {
        snprintf(appRenderer->state.announcementBuf,
                 sizeof(appRenderer->state.announcementBuf),
                 "%s", modeName);
    }

    appRenderer->state.announcement = appRenderer->state.announcementBuf;
    accesskitSpeak(appRenderer, appRenderer->state.announcementBuf);
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
    if (strncmp(label, "-cc ", 4) == 0 || strncmp(label, "+cc ", 4) == 0) return CHECKBOX_CHECKED;
    if (strncmp(label, "-c ", 3) == 0 || strncmp(label, "+c ", 3) == 0) return CHECKBOX_UNCHECKED;
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
            // Force line break on newline character
            if (*lineEnd == '\n') {
                break;
            }

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
        if (lineLen == 0 && *lineStart != '\0' && *lineStart != '\n') {
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

        // Skip newline character
        if (*lineStart == '\n') {
            lineStart++;
        }
        // Skip trailing space if we broke at one
        else if (*lineStart == ' ') {
            lineStart++;
        }
    }

    // Trailing newline means an empty line follows
    size_t textLen = strlen(text);
    if (textLen > 0 && text[textLen - 1] == '\n' && lineCount < 1000) {
        lines[lineCount].start = text + textLen;
        lines[lineCount].len = 0;
        lineCount++;
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
        }
        currentY += lineHeight;
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

static int countTextLines(const char *text) {
    if (!text || *text == '\0') return 1;
    int lines = 0;
    const char *seg = text;
    while (*seg != '\0') {
        const char *nl = strchr(seg, '\n');
        size_t segLen = nl ? (size_t)(nl - seg) : strlen(seg);
        if (segLen <= 120) {
            lines += 1;
        } else {
            lines += (int)((segLen + 119) / 120);
        }
        if (nl) {
            seg = nl + 1;
            if (*seg == '\0') lines++;
        } else {
            break;
        }
    }
    return lines > 0 ? lines : 1;
}

static int getItemLineCount(const ListItem *item, SiCompassApplication *app,
                            float charWidth, int lineHeight, int headerLines) {
    const char *label = item->label;
    if (label && strncmp(label, "-p ", 3) == 0) {
        const char *imagePath = item->data ? item->data : label + 3;
        const char *displayText = label;
        // Split display text into prefix and suffix around imagePath
        const char *pathInDisplay = item->data ? strstr(displayText, item->data) : NULL;
        int prefixLines = 0;
        int suffixLines = 0;
        if (pathInDisplay) {
            if (pathInDisplay > displayText) {
                size_t prefixLen = pathInDisplay - displayText;
                if (prefixLen > 3) {  // Skip prefix line for bare "-p " marker
                    char prefix[MAX_LINE_LENGTH] = {0};
                    if (prefixLen >= MAX_LINE_LENGTH) prefixLen = MAX_LINE_LENGTH - 1;
                    strncpy(prefix, displayText, prefixLen);
                    prefixLines = countTextLines(prefix);
                }
            }
            const char *suffix = pathInDisplay + strlen(item->data);
            if (suffix[0] != '\0') {
                suffixLines = countTextLines(suffix);
            }
        }

        if (loadImageTexture(app, imagePath)) {
            ImageRenderer *ir = app->imageRenderer;
            float imgW = (float)ir->textureWidth;
            float imgH = (float)ir->textureHeight;

            float maxW = charWidth * 120.0f;
            float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * (headerLines + prefixLines + suffixLines));

            float displayScale = 1.0f;
            if (imgW > maxW) displayScale = maxW / imgW;
            if (imgH * displayScale > maxH) displayScale = maxH / imgH;

            float displayH = imgH * displayScale;
            int imageLines = (int)ceilf(displayH / (float)lineHeight);
            if (imageLines < 1) imageLines = 1;

            return prefixLines + imageLines + suffixLines;
        }
    }
    return countTextLines(label);
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

    int linesToSelected;

    if (startIndex < 0) {
        // Sentinel: position listIndex as second-to-last by working backward
        int linesFromBottom = getItemLineCount(&list[listIndex], app, charWidth, lineHeight, headerLines);
        if (listIndex < count - 1) {
            linesFromBottom += getItemLineCount(&list[listIndex + 1], app, charWidth, lineHeight, headerLines);
        }
        startIndex = listIndex;
        while (startIndex > 0) {
            int prevLines = getItemLineCount(&list[startIndex - 1], app, charWidth, lineHeight, headerLines);
            if (linesFromBottom + prevLines > availableLines) break;
            linesFromBottom += prevLines;
            startIndex--;
        }
        linesToSelected = 0;
        for (int i = startIndex; i <= listIndex; i++) {
            linesToSelected += getItemLineCount(&list[i], app, charWidth, lineHeight, headerLines);
        }
    } else {
        // Normal scroll-into-view
        if (listIndex < startIndex) {
            startIndex = listIndex;
        }
        linesToSelected = 0;
        for (int i = startIndex; i <= listIndex; i++) {
            linesToSelected += getItemLineCount(&list[i], app, charWidth, lineHeight, headerLines);
        }
        while (linesToSelected > availableLines && startIndex < listIndex) {
            linesToSelected -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
            startIndex++;
        }
        // Try to show 1 item below selected if possible (scrolloff)
        if (listIndex < count - 1) {
            int nextLines = getItemLineCount(&list[listIndex + 1], app, charWidth, lineHeight, headerLines);
            int totalWithNext = linesToSelected + nextLines;
            int savedStartIndex = startIndex;
            int savedLinesToSelected = linesToSelected;
            while (totalWithNext > availableLines && startIndex < listIndex) {
                totalWithNext -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
                linesToSelected -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
                startIndex++;
            }
            // If the next item still doesn't fit, undo — don't sacrifice context above
            if (totalWithNext > availableLines) {
                startIndex = savedStartIndex;
                linesToSelected = savedLinesToSelected;
            }
        }
        // Try to show 1 item above selected if possible (scrolloff)
        if (startIndex > 0 && startIndex == listIndex) {
            int prevLines = getItemLineCount(&list[startIndex - 1], app, charWidth, lineHeight, headerLines);
            if (linesToSelected + prevLines <= availableLines) {
                startIndex--;
            }
        }
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    // Calculate endIndex: include items whose start position is within viewport
    int totalLines = 0;
    int endIndex = startIndex;
    while (endIndex < count) {
        if (totalLines >= availableLines) break;
        totalLines += getItemLineCount(&list[endIndex], app, charWidth, lineHeight, headerLines);
        endIndex++;
    }
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Check if this is an image item
        if (list[i].label && strncmp(list[i].label, "-p ", 3) == 0) {
            const char *imagePath = list[i].data ? list[i].data : list[i].label + 3;
            const char *displayText = list[i].label;

            // Split display text into prefix and suffix around imagePath
            const char *pathInDisplay = list[i].data ? strstr(displayText, list[i].data) : NULL;
            char imagePrefix[MAX_LINE_LENGTH] = {0};
            const char *imageSuffix = NULL;
            if (pathInDisplay) {
                size_t prefixLen = pathInDisplay - displayText;
                if (prefixLen > 0 && prefixLen < MAX_LINE_LENGTH) {
                    strncpy(imagePrefix, displayText, prefixLen);
                }
                const char *afterPath = pathInDisplay + strlen(list[i].data);
                if (afterPath[0] != '\0') imageSuffix = afterPath;
            }

            // Compute image position before drawing
            int suffixLineCount = imageSuffix ? countTextLines(imageSuffix) : 0;
            int prefixLineCount = (imagePrefix[0] && strlen(imagePrefix) > 3) ? countTextLines(imagePrefix) : 0;
            float ipMinX, ipMinY, ipMaxX, ipMaxY;
            calculateTextBounds(app, "-p ", (float)itemX, (float)itemYPos, scale,
                               &ipMinX, &ipMinY, &ipMaxX, &ipMaxY);
            float imgX = ipMaxX;
            float bgTop = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

            if (loadImageTexture(app, imagePath)) {
                ImageRenderer *ir = app->imageRenderer;
                float imgW = (float)ir->textureWidth;
                float imgH = (float)ir->textureHeight;

                // Calculate max display dimensions (accounting for prefix/suffix)
                float maxW = charWidth * 120.0f;
                float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * (headerLines + prefixLineCount + suffixLineCount));

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

                // Compute image Y after prefix lines advance itemYPos
                int renderItemYPos = itemYPos;
                if (prefixLineCount > 0) {
                    renderItemYPos = itemYPos + lineHeight * prefixLineCount;
                }
                float imgY = (float)renderItemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

                // Draw tight-fitting selection background
                if (isSelected) {
                    float bgLeft = (float)itemX - TEXT_PADDING;
                    float bgRight = imgX + displayW;
                    float bgBottom = imgY + displayH + (float)(suffixLineCount * lineHeight);
                    float bgW = bgRight - bgLeft;
                    float bgH = bgBottom - bgTop;
                    prepareRectangle(app, bgLeft, bgTop, bgW, bgH,
                                     app->appRenderer->palette->selected, 5.0f);
                    // Square off right corners where image edge meets background
                    if (prefixLineCount == 0) {
                        prepareRectangle(app, bgRight - 5.0f, bgTop, 5.0f, 5.0f,
                                         app->appRenderer->palette->selected, 0.0f);
                    }
                    if (suffixLineCount == 0) {
                        prepareRectangle(app, bgRight - 5.0f, bgTop + bgH - 5.0f, 5.0f, 5.0f,
                                         app->appRenderer->palette->selected, 0.0f);
                    }
                }

                // Render prefix above image, or bare "-p" inline with image
                if (imagePrefix[0] && strlen(imagePrefix) > 3) {
                    int prefixLines = renderText(app, imagePrefix, itemX, itemYPos,
                                                 app->appRenderer->palette->text, false);
                    yPos += lineHeight * prefixLines;
                    itemYPos = yPos;
                } else {
                    renderText(app, "-p", itemX, itemYPos, app->appRenderer->palette->text, false);
                }

                // Image inset by border thickness
                float border = 2.0f;
                prepareImage(app, imgX + border, imgY + border,
                             displayW - border * 2.0f, displayH - border * 2.0f);

                yPos = itemYPos + (int)ceilf(displayH);
            } else {
                // Failed to load image, show path as text
                int textLines = renderText(app, displayText, itemX, itemYPos, app->appRenderer->palette->text, isSelected);
                yPos += lineHeight * textLines;
            }

            // Render suffix below image
            if (imageSuffix) {
                int suffixLines = renderText(app, imageSuffix, itemX, yPos,
                                             app->appRenderer->palette->text, false);
                yPos += lineHeight * suffixLines;
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
            int baseItemX = itemX;

            // Render prefix without highlight
            if (app->appRenderer->inputPrefix[0] != '\0') {
                renderText(app, app->appRenderer->inputPrefix, itemX, itemYPos,
                           app->appRenderer->palette->text, false);
                float pfxMinX, pfxMinY, pfxMaxX, pfxMaxY;
                calculateTextBounds(app, app->appRenderer->inputPrefix,
                                   (float)itemX, (float)itemYPos, scale,
                                   &pfxMinX, &pfxMinY, &pfxMaxX, &pfxMaxY);
                itemX = (int)(pfxMaxX);
            }
            displayText = app->appRenderer->inputBuffer;

            // Store positions for caret rendering
            app->appRenderer->currentElementX = itemX;
            app->appRenderer->currentElementBaseX = baseItemX;
            app->appRenderer->currentElementY = itemYPos;

            // Multi-line input: split on \n, render first line at itemX, rest at baseItemX
            const char *nl = strchr(displayText, '\n');
            if (nl) {
                // Render first line at itemX (after prefix)
                char firstLine[MAX_LINE_LENGTH];
                size_t firstLen = nl - displayText;
                if (firstLen >= MAX_LINE_LENGTH) firstLen = MAX_LINE_LENGTH - 1;
                strncpy(firstLine, displayText, firstLen);
                firstLine[firstLen] = '\0';
                int textLines1 = renderText(app, firstLen > 0 ? firstLine : " ", itemX, itemYPos,
                                            app->appRenderer->palette->text, isSelected);

                // Render remaining lines at baseItemX, one line at a time
                const char *rest = nl + 1;
                int restY = itemYPos + lineHeight * textLines1;
                int textLinesRest = 0;
                if (*rest != '\0') {
                    const char *linePtr = rest;
                    while (*linePtr != '\0') {
                        const char *lineNl = strchr(linePtr, '\n');
                        char lineBuf[MAX_LINE_LENGTH];
                        size_t lineLen = lineNl ? (size_t)(lineNl - linePtr) : strlen(linePtr);
                        if (lineLen >= MAX_LINE_LENGTH) lineLen = MAX_LINE_LENGTH - 1;
                        strncpy(lineBuf, linePtr, lineLen);
                        lineBuf[lineLen] = '\0';
                        int curY = restY + lineHeight * textLinesRest;
                        textLinesRest += renderText(app, lineLen > 0 ? lineBuf : " ", baseItemX, curY,
                                                    app->appRenderer->palette->text, isSelected);
                        if (lineNl) {
                            linePtr = lineNl + 1;
                            if (*linePtr == '\0') {
                                curY = restY + lineHeight * textLinesRest;
                                textLinesRest += renderText(app, " ", baseItemX, curY,
                                                            app->appRenderer->palette->text, isSelected);
                            }
                        } else {
                            break;
                        }
                    }
                } else {
                    textLinesRest = renderText(app, " ", baseItemX, restY,
                                               app->appRenderer->palette->text, isSelected);
                }

                // Render suffix after the last line
                if (app->appRenderer->inputSuffix[0] != '\0') {
                    // Find the last line of content for suffix positioning
                    const char *lastNl = strrchr(displayText, '\n');
                    const char *lastLine = lastNl ? lastNl + 1 : displayText;
                    int lastLineY = restY + (textLinesRest > 1 ? lineHeight * (textLinesRest - 1) : 0);
                    int suffixBaseX = baseItemX;
                    if (textLinesRest == 0) {
                        lastLineY = itemYPos + lineHeight * (textLines1 - 1);
                        suffixBaseX = baseItemX;
                    }
                    float sfxMinX, sfxMinY, sfxMaxX, sfxMaxY;
                    calculateTextBounds(app, *lastLine ? lastLine : " ",
                                       (float)suffixBaseX, (float)lastLineY, scale,
                                       &sfxMinX, &sfxMinY, &sfxMaxX, &sfxMaxY);
                    int suffixX = (int)(sfxMaxX);
                    renderText(app, app->appRenderer->inputSuffix, suffixX, lastLineY,
                               app->appRenderer->palette->text, false);
                }

                yPos += lineHeight * (textLines1 + textLinesRest);
                continue;  // skip normal rendering below
            }
        }

        // Render text — highlight only the editable part in insert mode
        // For multiline labels, indent continuation lines past the list prefix
        const char *generalNl = strchr(displayText, '\n');
        int textLines;
        if (generalNl) {
            // Calculate continuation X from list prefix (e.g., "-i ")
            int contX = itemX;
            const char *sp = strchr(displayText, ' ');
            if (sp) {
                int pLen = (int)(sp - displayText + 1);
                char pStr[16];
                if (pLen > 15) pLen = 15;
                strncpy(pStr, displayText, pLen);
                pStr[pLen] = '\0';
                float pMinX, pMinY, pMaxX, pMaxY;
                calculateTextBounds(app, pStr, (float)itemX, (float)itemYPos, scale,
                                   &pMinX, &pMinY, &pMaxX, &pMaxY);
                contX = (int)pMaxX;
            }

            // Render first line at itemX (no highlight — unified rect drawn below)
            char firstLine[MAX_LINE_LENGTH];
            size_t firstLen = generalNl - displayText;
            if (firstLen >= MAX_LINE_LENGTH) firstLen = MAX_LINE_LENGTH - 1;
            strncpy(firstLine, displayText, firstLen);
            firstLine[firstLen] = '\0';
            int textLines1 = renderText(app, firstLen > 0 ? firstLine : " ", itemX, itemYPos,
                                        app->appRenderer->palette->text, false);

            // Render continuation lines at contX
            const char *rest = generalNl + 1;
            int restY = itemYPos + lineHeight * textLines1;
            int textLinesRest = 0;
            if (*rest != '\0') {
                textLinesRest = renderText(app, rest, contX, restY,
                                           app->appRenderer->palette->text, false);
            } else {
                textLinesRest = 1;
            }

            // Draw unified selection rectangle across all lines
            if (isSelected) {
                int totalLines = textLines1 + textLinesRest;
                float minX1, minY1, maxX1, maxY1;
                calculateTextBounds(app, firstLen > 0 ? firstLine : " ",
                                   (float)itemX, (float)itemYPos, scale,
                                   &minX1, &minY1, &maxX1, &maxY1);
                float oMinX = minX1, oMaxX = maxX1;

                // Measure continuation lines for max width
                if (*rest != '\0') {
                    const char *line = rest;
                    for (int li = 0; li < textLinesRest && *line; li++) {
                        const char *lnl = strchr(line, '\n');
                        char lineBuf[MAX_LINE_LENGTH];
                        size_t ll = lnl ? (size_t)(lnl - line) : strlen(line);
                        if (ll >= MAX_LINE_LENGTH) ll = MAX_LINE_LENGTH - 1;
                        strncpy(lineBuf, line, ll);
                        lineBuf[ll] = '\0';
                        if (ll > 0) {
                            float lMinX, lMinY, lMaxX, lMaxY;
                            calculateTextBounds(app, lineBuf, (float)contX,
                                               (float)(restY + li * lineHeight), scale,
                                               &lMinX, &lMinY, &lMaxX, &lMaxY);
                            if (lMinX < oMinX) oMinX = lMinX;
                            if (lMaxX > oMaxX) oMaxX = lMaxX;
                        }
                        line = lnl ? lnl + 1 : line + ll;
                    }
                }

                float rectY = minY1 - TEXT_PADDING;
                float rectH = (float)(totalLines * lineHeight);
                prepareRectangle(app, oMinX - TEXT_PADDING, rectY,
                                 oMaxX - oMinX + 2 * TEXT_PADDING, rectH,
                                 app->appRenderer->palette->selected, 5.0f);
            }

            textLines = textLines1 + textLinesRest;
        } else {
            textLines = renderText(app, displayText, itemX, itemYPos, app->appRenderer->palette->text,
                                       isSelected);
        }

        // Render non-editable suffix without highlight
        if (isSelected && inInsertMode && app->appRenderer->inputSuffix[0] != '\0') {
            float sfxMinX, sfxMinY, sfxMaxX, sfxMaxY;
            calculateTextBounds(app, displayText,
                               (float)itemX, (float)itemYPos, scale,
                               &sfxMinX, &sfxMinY, &sfxMaxX, &sfxMaxY);
            int suffixX = (int)(sfxMaxX);
            renderText(app, app->appRenderer->inputSuffix, suffixX, itemYPos,
                       app->appRenderer->palette->text, false);
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

    int linesToSelected;

    if (startIndex < 0) {
        // Sentinel: position listIndex as second-to-last by working backward
        int linesFromBottom = getItemLineCount(&list[listIndex], app, charWidth, lineHeight, headerLines);
        if (listIndex < count - 1) {
            linesFromBottom += getItemLineCount(&list[listIndex + 1], app, charWidth, lineHeight, headerLines);
        }
        startIndex = listIndex;
        while (startIndex > 0) {
            int prevLines = getItemLineCount(&list[startIndex - 1], app, charWidth, lineHeight, headerLines);
            if (linesFromBottom + prevLines > availableLines) break;
            linesFromBottom += prevLines;
            startIndex--;
        }
        linesToSelected = 0;
        for (int i = startIndex; i <= listIndex; i++) {
            linesToSelected += getItemLineCount(&list[i], app, charWidth, lineHeight, headerLines);
        }
    } else {
        // Normal scroll-into-view
        if (listIndex < startIndex) {
            startIndex = listIndex;
        }
        linesToSelected = 0;
        for (int i = startIndex; i <= listIndex; i++) {
            linesToSelected += getItemLineCount(&list[i], app, charWidth, lineHeight, headerLines);
        }
        while (linesToSelected > availableLines && startIndex < listIndex) {
            linesToSelected -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
            startIndex++;
        }
        // Try to show 1 item below selected if possible (scrolloff)
        if (listIndex < count - 1) {
            int nextLines = getItemLineCount(&list[listIndex + 1], app, charWidth, lineHeight, headerLines);
            int totalWithNext = linesToSelected + nextLines;
            int savedStartIndex = startIndex;
            int savedLinesToSelected = linesToSelected;
            while (totalWithNext > availableLines && startIndex < listIndex) {
                totalWithNext -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
                linesToSelected -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
                startIndex++;
            }
            // If the next item still doesn't fit, undo — don't sacrifice context above
            if (totalWithNext > availableLines) {
                startIndex = savedStartIndex;
                linesToSelected = savedLinesToSelected;
            }
        }
        // Try to show 1 item above selected if possible (scrolloff)
        if (startIndex > 0 && startIndex == listIndex) {
            int prevLines = getItemLineCount(&list[startIndex - 1], app, charWidth, lineHeight, headerLines);
            if (linesToSelected + prevLines <= availableLines) {
                startIndex--;
            }
        }
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    // Calculate endIndex: include items whose start position is within viewport
    int totalLines = 0;
    int endIndex = startIndex;
    while (endIndex < count) {
        if (totalLines >= availableLines) break;
        totalLines += getItemLineCount(&list[endIndex], app, charWidth, lineHeight, headerLines);
        endIndex++;
    }
    if (endIndex > count) endIndex = count;

    for (int i = startIndex; i < endIndex; i++) {
        bool isSelected = (i == app->appRenderer->listIndex);
        int itemYPos = yPos;
        int itemX = 50 + indent;

        // Check if this is an image item
        if (list[i].label && strncmp(list[i].label, "-p ", 3) == 0) {
            const char *imagePath = list[i].data ? list[i].data : list[i].label + 3;
            const char *displayText = list[i].label;

            // Split display text into prefix and suffix around imagePath
            const char *pathInDisplay = list[i].data ? strstr(displayText, list[i].data) : NULL;
            char imagePrefix[MAX_LINE_LENGTH] = {0};
            const char *imageSuffix = NULL;
            if (pathInDisplay) {
                size_t prefixLen = pathInDisplay - displayText;
                if (prefixLen > 0 && prefixLen < MAX_LINE_LENGTH) {
                    strncpy(imagePrefix, displayText, prefixLen);
                }
                const char *afterPath = pathInDisplay + strlen(list[i].data);
                if (afterPath[0] != '\0') imageSuffix = afterPath;
            }

            // Compute image position before drawing
            int suffixLineCount = imageSuffix ? countTextLines(imageSuffix) : 0;
            int prefixLineCount = (imagePrefix[0] && strlen(imagePrefix) > 3) ? countTextLines(imagePrefix) : 0;
            float ipMinX, ipMinY, ipMaxX, ipMaxY;
            calculateTextBounds(app, "-p ", (float)itemX, (float)itemYPos, scale,
                               &ipMinX, &ipMinY, &ipMaxX, &ipMaxY);
            float imgX = ipMaxX;
            float bgTop = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

            if (loadImageTexture(app, imagePath)) {
                ImageRenderer *ir = app->imageRenderer;
                float imgW = (float)ir->textureWidth;
                float imgH = (float)ir->textureHeight;

                float maxW = charWidth * 120.0f;
                float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * (headerLines + prefixLineCount + suffixLineCount));

                float displayScale = 1.0f;
                if (imgW > maxW) displayScale = maxW / imgW;
                if (imgH * displayScale > maxH) displayScale = maxH / imgH;

                float displayW = imgW * displayScale;
                float displayH = imgH * displayScale;

                int renderItemYPos = itemYPos;
                if (prefixLineCount > 0) {
                    renderItemYPos = itemYPos + lineHeight * prefixLineCount;
                }
                float imgY = (float)renderItemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

                // Draw tight-fitting selection background
                if (isSelected) {
                    float bgLeft = (float)itemX - TEXT_PADDING;
                    float bgRight = imgX + displayW;
                    float bgBottom = imgY + displayH + (float)(suffixLineCount * lineHeight);
                    float bgW = bgRight - bgLeft;
                    float bgH = bgBottom - bgTop;
                    prepareRectangle(app, bgLeft, bgTop, bgW, bgH,
                                     app->appRenderer->palette->selected, 5.0f);
                    // Square off right corners where image edge meets background
                    if (prefixLineCount == 0) {
                        prepareRectangle(app, bgRight - 5.0f, bgTop, 5.0f, 5.0f,
                                         app->appRenderer->palette->selected, 0.0f);
                    }
                    if (suffixLineCount == 0) {
                        prepareRectangle(app, bgRight - 5.0f, bgTop + bgH - 5.0f, 5.0f, 5.0f,
                                         app->appRenderer->palette->selected, 0.0f);
                    }
                }

                // Render prefix above image, or bare "-p" inline with image
                if (imagePrefix[0] && strlen(imagePrefix) > 3) {
                    int prefixLines = renderText(app, imagePrefix, itemX, itemYPos,
                                                 app->appRenderer->palette->text, false);
                    yPos += lineHeight * prefixLines;
                    itemYPos = yPos;
                } else {
                    renderText(app, "-p", itemX, itemYPos, app->appRenderer->palette->text, false);
                }

                float border = 2.0f;
                prepareImage(app, imgX + border, imgY + border,
                             displayW - border * 2.0f, displayH - border * 2.0f);

                yPos = itemYPos + (int)ceilf(displayH);
            } else {
                int textLines = renderText(app, displayText, itemX, itemYPos, app->appRenderer->palette->text, isSelected);
                yPos += lineHeight * textLines;
            }

            // Render suffix below image
            if (imageSuffix) {
                int suffixLines = renderText(app, imageSuffix, itemX, yPos,
                                             app->appRenderer->palette->text, false);
                yPos += lineHeight * suffixLines;
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

    int linesToSelected;

    if (startIndex < 0) {
        // Sentinel: position listIndex as second-to-last by working backward
        int linesFromBottom = getItemLineCount(&list[listIndex], app, charWidth, lineHeight, headerLines);
        if (listIndex < count - 1) {
            linesFromBottom += getItemLineCount(&list[listIndex + 1], app, charWidth, lineHeight, headerLines);
        }
        startIndex = listIndex;
        while (startIndex > 0) {
            int prevLines = getItemLineCount(&list[startIndex - 1], app, charWidth, lineHeight, headerLines);
            if (linesFromBottom + prevLines > availableLines) break;
            linesFromBottom += prevLines;
            startIndex--;
        }
        linesToSelected = 0;
        for (int i = startIndex; i <= listIndex; i++) {
            linesToSelected += getItemLineCount(&list[i], app, charWidth, lineHeight, headerLines);
        }
    } else {
        // Normal scroll-into-view
        if (listIndex < startIndex) {
            startIndex = listIndex;
        }
        linesToSelected = 0;
        for (int i = startIndex; i <= listIndex; i++) {
            linesToSelected += getItemLineCount(&list[i], app, charWidth, lineHeight, headerLines);
        }
        while (linesToSelected > availableLines && startIndex < listIndex) {
            linesToSelected -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
            startIndex++;
        }
        // Try to show 1 item below selected if possible (scrolloff)
        if (listIndex < count - 1) {
            int nextLines = getItemLineCount(&list[listIndex + 1], app, charWidth, lineHeight, headerLines);
            int totalWithNext = linesToSelected + nextLines;
            int savedStartIndex = startIndex;
            int savedLinesToSelected = linesToSelected;
            while (totalWithNext > availableLines && startIndex < listIndex) {
                totalWithNext -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
                linesToSelected -= getItemLineCount(&list[startIndex], app, charWidth, lineHeight, headerLines);
                startIndex++;
            }
            // If the next item still doesn't fit, undo — don't sacrifice context above
            if (totalWithNext > availableLines) {
                startIndex = savedStartIndex;
                linesToSelected = savedLinesToSelected;
            }
        }
        // Try to show 1 item above selected if possible (scrolloff)
        if (startIndex > 0 && startIndex == listIndex) {
            int prevLines = getItemLineCount(&list[startIndex - 1], app, charWidth, lineHeight, headerLines);
            if (linesToSelected + prevLines <= availableLines) {
                startIndex--;
            }
        }
    }

    if (startIndex < 0) startIndex = 0;
    app->appRenderer->scrollOffset = startIndex;

    // Calculate endIndex: include items whose start position is within viewport
    int totalLines = 0;
    int endIndex = startIndex;
    while (endIndex < count) {
        if (totalLines >= availableLines) break;
        totalLines += getItemLineCount(&list[endIndex], app, charWidth, lineHeight, headerLines);
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
            const char *imagePath = list[i].data ? list[i].data : list[i].label + 3;
            const char *displayText = list[i].label;

            // Split display text into prefix and suffix around imagePath
            const char *pathInDisplay = list[i].data ? strstr(displayText, list[i].data) : NULL;
            char imagePrefix[MAX_LINE_LENGTH] = {0};
            const char *imageSuffix = NULL;
            if (pathInDisplay) {
                size_t prefixLen = pathInDisplay - displayText;
                if (prefixLen > 0 && prefixLen < MAX_LINE_LENGTH) {
                    strncpy(imagePrefix, displayText, prefixLen);
                }
                const char *afterPath = pathInDisplay + strlen(list[i].data);
                if (afterPath[0] != '\0') imageSuffix = afterPath;
            }

            // Compute image position before drawing
            int suffixLineCount = imageSuffix ? countTextLines(imageSuffix) : 0;
            int prefixLineCount = (imagePrefix[0] && strlen(imagePrefix) > 3) ? countTextLines(imagePrefix) : 0;
            float ipMinX, ipMinY, ipMaxX, ipMaxY;
            calculateTextBounds(app, "-p ", (float)itemX, (float)itemYPos, scale,
                               &ipMinX, &ipMinY, &ipMaxX, &ipMaxY);
            float imgX = ipMaxX;
            float bgTop = (float)itemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

            if (loadImageTexture(app, imagePath)) {
                ImageRenderer *ir = app->imageRenderer;
                float imgW = (float)ir->textureWidth;
                float imgH = (float)ir->textureHeight;

                float maxW = charWidth * 120.0f;
                float maxH = (float)app->swapChainExtent.height - (float)(lineHeight * (headerLines + prefixLineCount + suffixLineCount));

                float displayScale = 1.0f;
                if (imgW > maxW) displayScale = maxW / imgW;
                if (imgH * displayScale > maxH) displayScale = maxH / imgH;

                float displayW = imgW * displayScale;
                float displayH = imgH * displayScale;

                int renderItemYPos = itemYPos;
                if (prefixLineCount > 0) {
                    renderItemYPos = itemYPos + lineHeight * prefixLineCount;
                }
                float imgY = (float)renderItemYPos - app->fontRenderer->ascender * scale - TEXT_PADDING;

                // Draw tight-fitting selection background
                if (isSelected) {
                    float bgLeft = (float)itemX - TEXT_PADDING;
                    float bgRight = imgX + displayW;
                    float bgBottom = imgY + displayH + (float)(suffixLineCount * lineHeight);
                    float bgW = bgRight - bgLeft;
                    float bgH = bgBottom - bgTop;
                    prepareRectangle(app, bgLeft, bgTop, bgW, bgH,
                                     app->appRenderer->palette->selected, 5.0f);
                    // Square off right corners where image edge meets background
                    if (prefixLineCount == 0) {
                        prepareRectangle(app, bgRight - 5.0f, bgTop, 5.0f, 5.0f,
                                         app->appRenderer->palette->selected, 0.0f);
                    }
                    if (suffixLineCount == 0) {
                        prepareRectangle(app, bgRight - 5.0f, bgTop + bgH - 5.0f, 5.0f, 5.0f,
                                         app->appRenderer->palette->selected, 0.0f);
                    }
                }

                // Render prefix above image, or bare "-p" inline with image
                if (imagePrefix[0] && strlen(imagePrefix) > 3) {
                    int prefixLines = renderText(app, imagePrefix, itemX, itemYPos,
                                                 app->appRenderer->palette->text, false);
                    yPos += lineHeight * prefixLines;
                    itemYPos = yPos;
                } else {
                    renderText(app, "-p", itemX, itemYPos, app->appRenderer->palette->text, false);
                }

                float border = 2.0f;
                prepareImage(app, imgX + border, imgY + border,
                             displayW - border * 2.0f, displayH - border * 2.0f);

                yPos = itemYPos + (int)ceilf(displayH);
            } else {
                int textLines = renderText(app, displayText, itemX, itemYPos, app->appRenderer->palette->text, isSelected);
                yPos += lineHeight * textLines;
            }

            // Render suffix below image
            if (imageSuffix) {
                int suffixLines = renderText(app, imageSuffix, itemX, yPos,
                                             app->appRenderer->palette->text, false);
                yPos += lineHeight * suffixLines;
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

                // Extract prefix (text before <image>) and suffix (text after </image>)
                const char *imgTagOpen = strstr(text, IMAGE_TAG_OPEN);
                const char *imgTagClose = imgTagOpen ? strstr(imgTagOpen, IMAGE_TAG_CLOSE) : NULL;
                char imagePrefix[MAX_LINE_LENGTH] = {0};
                const char *imageSuffix = NULL;
                int prefixLines = 0;
                int suffixLines = 0;
                if (imgTagOpen && imgTagClose) {
                    size_t prefixLen = imgTagOpen - text;
                    if (prefixLen > 0 && prefixLen < MAX_LINE_LENGTH) {
                        strncpy(imagePrefix, text, prefixLen);
                        char *stripped = providerTagStripDisplay(imagePrefix);
                        if (stripped) {
                            strncpy(imagePrefix, stripped, MAX_LINE_LENGTH - 1);
                            free(stripped);
                        }
                        if (imagePrefix[0]) prefixLines = countTextLines(imagePrefix);
                    }
                    const char *afterClose = imgTagClose + IMAGE_TAG_CLOSE_LEN;
                    if (afterClose[0] != '\0') {
                        imageSuffix = afterClose;
                        suffixLines = countTextLines(imageSuffix);
                    }
                }

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

                    // Render prefix text above image
                    if (prefixLines > 0) {
                        renderText(app, imagePrefix, 50, yPos, app->appRenderer->palette->text, false);
                    }

                    float imgX = 50.0f;
                    float imgY = 1.5f * (float)lineHeight - (float)(app->appRenderer->textScrollOffset * lineHeight);
                    if (prefixLines > 0) {
                        imgY += (float)(prefixLines * lineHeight);
                    }

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

                    // Render suffix text below image (match prefix-to-image gap)
                    if (imageSuffix) {
                        float textImageGap = 0.5f * (app->fontRenderer->ascender + app->fontRenderer->descender) * scale;
                        int suffixYPos = (int)(imgY + displayH + textImageGap) + (int)(app->fontRenderer->ascender * scale) + TEXT_PADDING;
                        char *strippedSuffix = providerTagStripDisplay(imageSuffix);
                        renderText(app, strippedSuffix ? strippedSuffix : imageSuffix, 50, suffixYPos, app->appRenderer->palette->text, false);
                        free(strippedSuffix);
                    }

                    int imageLines = (int)ceilf(displayH / (float)lineHeight);
                    if (imageLines < 1) imageLines = 1;
                    int totalLines = prefixLines + imageLines + suffixLines;
                    app->appRenderer->textScrollLineCount = totalLines;
                } else {
                    int lines = renderText(app, imagePath ? imagePath : text, 50, yPos, app->appRenderer->palette->text, false);
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

    // Build searchable text: for images, combine prefix+suffix; otherwise strip tags
    char *imagePath = NULL;
    int imagePixelLines = 0;
    float imageDisplayW = 0, imageDisplayH = 0;
    int prefixWrappedLineCount = 0;
    bool hasImage = providerTagHasImage(rawText);

    if (hasImage) {
        imagePath = providerTagExtractImageContent(rawText);

        // Extract prefix/suffix and build combined searchable text
        const char *imgTagOpen = strstr(rawText, IMAGE_TAG_OPEN);
        const char *imgTagClose = imgTagOpen ? strstr(imgTagOpen, IMAGE_TAG_CLOSE) : NULL;
        char combinedText[MAX_LINE_LENGTH * 2] = {0};

        if (imgTagOpen && imgTagClose) {
            size_t prefixLen = imgTagOpen - rawText;
            if (prefixLen > 0 && prefixLen < MAX_LINE_LENGTH) {
                char prefixBuf[MAX_LINE_LENGTH] = {0};
                strncpy(prefixBuf, rawText, prefixLen);
                char *strippedPfx = providerTagStripDisplay(prefixBuf);
                const char *pfx = strippedPfx ? strippedPfx : prefixBuf;
                if (pfx[0]) {
                    strcpy(combinedText, pfx);
                    strcat(combinedText, "\n");
                }
                free(strippedPfx);
            }
            const char *afterClose = imgTagClose + IMAGE_TAG_CLOSE_LEN;
            if (afterClose[0] != '\0') {
                char *strippedSfx = providerTagStripDisplay(afterClose);
                strcat(combinedText, strippedSfx ? strippedSfx : afterClose);
                free(strippedSfx);
            }
        }

        stripped = strdup(combinedText);

        // Load image dimensions
        if (imagePath && loadImageTexture(app, imagePath)) {
            ImageRenderer *ir = app->imageRenderer;
            float imgW = (float)ir->textureWidth;
            float imgH = (float)ir->textureHeight;
            float imgMaxW = charWidth * 120.0f;
            float ds = 1.0f;
            if (imgW > imgMaxW) ds = imgMaxW / imgW;
            imageDisplayW = imgW * ds;
            imageDisplayH = imgH * ds;
            imagePixelLines = (int)ceilf(imageDisplayH / (float)lineHeight);
            if (imagePixelLines < 1) imagePixelLines = 1;
        }
    } else {
        stripped = providerTagStripDisplay(rawText);
    }
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
            if (*lineEnd == '\n') break;

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
        if (lineLen == 0 && *lineStart != '\0' && *lineStart != '\n') {
            lineLen = 1;
            lineEnd = lineStart + 1;
        }
        if (lineLen >= MAX_LINE_LENGTH) lineLen = MAX_LINE_LENGTH - 1;

        lines[lineCount].start = lineStart;
        lines[lineCount].len = lineLen;
        lines[lineCount].byteOffset = (int)(lineStart - text);
        lineCount++;

        lineStart = lineEnd;
        if (*lineStart == '\n') lineStart++;
        else if (*lineStart == ' ') lineStart++;
    }

    // For images, determine which wrapped lines are prefix vs suffix
    if (hasImage) {
        for (int i = 0; i < lineCount; i++) {
            // Lines whose content ends at or before the newline separator are prefix
            if (lines[i].byteOffset < (int)strlen(text) &&
                (lines[i].start + lines[i].len <= strchr(text, '\n') || strchr(text, '\n') == NULL)) {
                prefixWrappedLineCount = i + 1;
            } else {
                break;
            }
        }
        // If no newline found (prefix only or suffix only), all lines are prefix
        if (!strchr(text, '\n')) prefixWrappedLineCount = lineCount;
        app->appRenderer->textScrollLineCount = lineCount + imagePixelLines;
    } else {
        app->appRenderer->textScrollLineCount = lineCount;
    }

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

    // Auto-scroll to current match (account for image lines in virtual line space)
    if (matchCount > 0) {
        int currentIdx = app->appRenderer->scrollSearchCurrentMatch;
        int matchLine = matches[currentIdx].wrappedLine;
        // Convert to virtual line (offset suffix lines by image height)
        int virtualLine = matchLine;
        if (hasImage && matchLine >= prefixWrappedLineCount) {
            virtualLine = matchLine + imagePixelLines;
        }

        int headerLines = 3;
        int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
        int visibleLines = availableHeight / lineHeight;
        if (visibleLines < 1) visibleLines = 1;

        if (virtualLine < app->appRenderer->textScrollOffset) {
            app->appRenderer->textScrollOffset = virtualLine;
        } else if (virtualLine >= app->appRenderer->textScrollOffset + visibleLines) {
            app->appRenderer->textScrollOffset = virtualLine - visibleLines + 1;
        }

        int totalVirtualLines = hasImage ? lineCount + imagePixelLines : lineCount;
        int maxOffset = totalVirtualLines - visibleLines;
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

    // Render text with highlights (and image for image elements)
    int textStartY = lineHeight * 3;
    app->appRenderer->renderClipTopY = textStartY;

    // Render image between prefix and suffix lines
    if (hasImage && imagePath && imagePixelLines > 0) {
        float imgX = 50.0f;
        float imgY = (float)textStartY + (float)(prefixWrappedLineCount - app->appRenderer->textScrollOffset) * (float)lineHeight;
        if (prefixWrappedLineCount > 0) {
            imgY -= 0.5f * (float)lineHeight;
        }
        prepareImage(app, imgX, imgY, imageDisplayW, imageDisplayH);

        // Clip top edge at bottom of search bar line
        float clipTop = 2.5f * (float)lineHeight;
        if (imgY < clipTop && app->imageRenderer->drawCallCount > 0) {
            float uvTop = (clipTop - imgY) / imageDisplayH;
            ImageDrawCall *dc = &app->imageRenderer->drawCalls[app->imageRenderer->drawCallCount - 1];
            dc->vertices[0].pos[1] = clipTop;
            dc->vertices[0].texCoord[1] = uvTop;
            dc->vertices[1].pos[1] = clipTop;
            dc->vertices[1].texCoord[1] = uvTop;
            dc->vertices[3].pos[1] = clipTop;
            dc->vertices[3].texCoord[1] = uvTop;
        }
    }
    free(imagePath);

    for (int i = 0; i < lineCount; i++) {
        // Compute Y position: suffix lines positioned relative to actual image bottom
        int currentY;
        if (hasImage && i >= prefixWrappedLineCount) {
            float imgYBase = (float)textStartY + (float)(prefixWrappedLineCount - app->appRenderer->textScrollOffset) * (float)lineHeight;
            if (prefixWrappedLineCount > 0) imgYBase -= 0.5f * (float)lineHeight;
            float textImageGap = 0.5f * (app->fontRenderer->ascender + app->fontRenderer->descender) * scale;
            int suffixBaseY = (int)(imgYBase + imageDisplayH + textImageGap) + (int)(app->fontRenderer->ascender * scale) + TEXT_PADDING;
            currentY = suffixBaseY + (i - prefixWrappedLineCount) * lineHeight;
        } else {
            currentY = textStartY + (i - app->appRenderer->textScrollOffset) * lineHeight;
        }

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

void renderInputSearch(SiCompassApplication *app) {
    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);
    float charWidth = getWidthEM(app, scale);
    float maxWidth = charWidth * 120.0f;

    // Text source is the saved input buffer
    const char *text = app->appRenderer->savedInputBuffer;
    if (!text || text[0] == '\0') {
        char searchDisplay[MAX_LINE_LENGTH];
        snprintf(searchDisplay, sizeof(searchDisplay), "search: %s [0 items]",
                 app->appRenderer->inputBuffer);
        renderText(app, searchDisplay, 50, lineHeight * 2, app->appRenderer->palette->text, false);
        app->appRenderer->inputSearchMatchCount = 0;
        app->appRenderer->inputSearchCurrentMatch = 0;
        return;
    }

    // Line-wrap the text
    typedef struct {
        const char *start;
        size_t len;
        int byteOffset;
    } LineInfo;

    LineInfo lines[1000];
    int lineCount = 0;
    const char *lineStart = text;

    while (*lineStart != '\0' && lineCount < 1000) {
        // Handle explicit newlines
        if (*lineStart == '\n') {
            lines[lineCount].start = lineStart;
            lines[lineCount].len = 0;
            lines[lineCount].byteOffset = (int)(lineStart - text);
            lineCount++;
            lineStart++;
            continue;
        }

        const char *lineEnd = lineStart;
        const char *lastSpace = NULL;
        const char *lastFit = lineStart;
        int currentY = lineHeight * 3 + lineCount * lineHeight;

        while (*lineEnd != '\0' && *lineEnd != '\n') {
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
        if (lineLen == 0 && *lineStart != '\0' && *lineStart != '\n') {
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

    app->appRenderer->inputSearchScrollLineCount = lineCount;

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
            int matchLen = searchLen;

            matches[matchCount].byteOffset = byteOffset;
            matches[matchCount].length = matchLen;
            matches[matchCount].wrappedLine = 0;
            matches[matchCount].lineLocalByte = 0;

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
            pos = found + 1;
        }
    }

    app->appRenderer->inputSearchMatchCount = matchCount;
    if (app->appRenderer->inputSearchCurrentMatch >= matchCount) {
        app->appRenderer->inputSearchCurrentMatch = matchCount > 0 ? matchCount - 1 : 0;
    }

    // Auto-scroll to current match
    if (matchCount > 0) {
        int currentIdx = app->appRenderer->inputSearchCurrentMatch;
        int matchLine = matches[currentIdx].wrappedLine;

        int headerLines = 3;
        int availableHeight = (int)app->swapChainExtent.height - (lineHeight * headerLines);
        int visibleLines = availableHeight / lineHeight;
        if (visibleLines < 1) visibleLines = 1;

        if (matchLine < app->appRenderer->inputSearchScrollOffset) {
            app->appRenderer->inputSearchScrollOffset = matchLine;
        } else if (matchLine >= app->appRenderer->inputSearchScrollOffset + visibleLines) {
            app->appRenderer->inputSearchScrollOffset = matchLine - visibleLines + 1;
        }

        int maxOffset = lineCount - visibleLines;
        if (maxOffset < 0) maxOffset = 0;
        if (app->appRenderer->inputSearchScrollOffset > maxOffset)
            app->appRenderer->inputSearchScrollOffset = maxOffset;
        if (app->appRenderer->inputSearchScrollOffset < 0)
            app->appRenderer->inputSearchScrollOffset = 0;
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
        int currentY = textStartY + (i - app->appRenderer->inputSearchScrollOffset) * lineHeight;

        if (currentY + lineHeight < textStartY) continue;
        if (currentY > (int)app->swapChainExtent.height) break;

        // Render match highlight rectangles for this line
        for (int m = 0; m < matchCount; m++) {
            if (matches[m].wrappedLine != i) continue;
            if (currentY < textStartY) continue;

            int localStart = matches[m].lineLocalByte;
            int localEnd = localStart + matches[m].length;
            if (localEnd > (int)lines[i].len) localEnd = (int)lines[i].len;

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

            int matchLen = localEnd - localStart;
            char matchStr[MAX_LINE_LENGTH];
            strncpy(matchStr, lines[i].start + localStart, matchLen);
            matchStr[matchLen] = '\0';

            float mMinX, mMinY, mMaxX, mMaxY;
            calculateTextBounds(app, matchStr, matchX, (float)currentY, scale,
                                &mMinX, &mMinY, &mMaxX, &mMaxY);

            uint32_t highlightColor = (m == app->appRenderer->inputSearchCurrentMatch)
                ? app->appRenderer->palette->scrollsearch : app->appRenderer->palette->selected;

            float rectY = mMinY - TEXT_PADDING;
            float rectH = getLineHeight(app, scale, TEXT_PADDING);
            float rectW = mMaxX - matchX;
            prepareRectangle(app, matchX, rectY, rectW, rectH, highlightColor, 3.0f);
        }

        // Render the text itself
        char lineText[MAX_LINE_LENGTH];
        if (lines[i].len > 0) {
            strncpy(lineText, lines[i].start, lines[i].len);
            lineText[lines[i].len] = '\0';
        } else {
            lineText[0] = '\0';
        }

        if (lines[i].len > 0 && currentY >= textStartY) {
            prepareTextForRendering(app, lineText, 50.0f, (float)currentY, scale, app->appRenderer->palette->text);
        }
    }

    app->appRenderer->renderClipTopY = 0;
}

void renderDashboard(SiCompassApplication *app) {
    const char *path = app->appRenderer->dashboardImagePath;
    if (!path || path[0] == '\0') return;

    if (!loadImageTexture(app, path)) return;

    ImageRenderer *ir = app->imageRenderer;
    float imgW = (float)ir->textureWidth;
    float imgH = (float)ir->textureHeight;
    float screenW = (float)app->swapChainExtent.width;
    float screenH = (float)app->swapChainExtent.height;

    float scale = getTextScale(app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(app, scale, TEXT_PADDING);

    // Available area below header
    float availW = screenW - 100.0f;
    float availH = screenH - (float)(lineHeight * 2);

    // Fit image maintaining aspect ratio
    float displayScale = 1.0f;
    if (imgW > availW) displayScale = availW / imgW;
    if (imgH * displayScale > availH) displayScale = availH / imgH;

    float displayW = imgW * displayScale;
    float displayH = imgH * displayScale;

    // Center on screen below header
    float imgX = (screenW - displayW) / 2.0f;
    float imgY = (float)(lineHeight) + (availH - displayH) / 2.0f;

    prepareImage(app, imgX, imgY, displayW, displayH);
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

    // Cache layout metrics for handler use (scroll, page up/down)
    app->appRenderer->windowHeight = (int)app->swapChainExtent.height;
    app->appRenderer->cachedLineHeight = lineHeight;

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
    if (app->appRenderer->currentCoordinate == COORDINATE_DASHBOARD) {
        renderDashboard(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        renderScroll(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        renderScrollSearch(app);
    } else if (app->appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
        renderInputSearch(app);
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
        app->appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH ||
        app->appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
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
                   50 + searchPrefixWidth,
                   app->appRenderer->cursorPosition,
                   app->appRenderer->palette->text);
    }

    // Render selection highlight in all text input modes
    if (hasSelection(app->appRenderer)) {
        int selStart, selEnd;
        getSelectionRange(app->appRenderer, &selStart, &selEnd);
        const char *buf = app->appRenderer->inputBuffer;
        uint32_t selColor = app->appRenderer->palette->scrollsearch;
        float selHeight = getLineHeight(app, scale, TEXT_PADDING) - (2.0f * TEXT_PADDING);

        int baseX, baseY;
        bool isInsertMode = (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                             app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT);

        if (!isInsertMode) {
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
            baseX = app->appRenderer->currentElementX;
            baseY = app->appRenderer->currentElementY;
        }

        // Find line boundaries for selStart and selEnd
        // Build line start offsets
        int lineStarts[1000];
        int numLines = 0;
        lineStarts[numLines++] = 0;
        for (int i = 0; i < app->appRenderer->inputBufferSize && numLines < 1000; i++) {
            if (buf[i] == '\n') lineStarts[numLines++] = i + 1;
        }

        // Find which line selStart and selEnd are on
        int startLine = 0, endLine = 0;
        for (int l = numLines - 1; l >= 0; l--) {
            if (lineStarts[l] <= selStart) { startLine = l; break; }
        }
        for (int l = numLines - 1; l >= 0; l--) {
            if (lineStarts[l] <= selEnd) { endLine = l; break; }
        }

        int elementX = app->appRenderer->currentElementX;
        int elementBaseX = app->appRenderer->currentElementBaseX;

        for (int line = startLine; line <= endLine; line++) {
            int lineX = isInsertMode ? ((line == 0) ? elementX : elementBaseX) : baseX;
            int lineY = baseY + line * lineHeight;
            int lineStartOff = lineStarts[line];
            int lineEndOff = (line + 1 < numLines) ? lineStarts[line + 1] - 1 : app->appRenderer->inputBufferSize;

            // Clamp selection to this line
            int clampStart = (selStart > lineStartOff) ? selStart : lineStartOff;
            int clampEnd = (selEnd < lineEndOff) ? selEnd : lineEndOff;

            // Calculate X start
            float xStart = (float)lineX;
            if (clampStart > lineStartOff) {
                int len = clampStart - lineStartOff;
                char tmp[MAX_LINE_LENGTH];
                if (len >= MAX_LINE_LENGTH) len = MAX_LINE_LENGTH - 1;
                strncpy(tmp, buf + lineStartOff, len);
                tmp[len] = '\0';
                float minX, minY, maxX, maxY;
                calculateTextBounds(app, tmp, (float)lineX, (float)lineY, scale,
                                    &minX, &minY, &maxX, &maxY);
                xStart = maxX;
            }

            // Calculate X end
            float xEnd = xStart;
            int len2 = clampEnd - lineStartOff;
            if (len2 > 0) {
                char tmp2[MAX_LINE_LENGTH];
                if (len2 >= MAX_LINE_LENGTH) len2 = MAX_LINE_LENGTH - 1;
                strncpy(tmp2, buf + lineStartOff, len2);
                tmp2[len2] = '\0';
                float minX, minY, maxX, maxY;
                calculateTextBounds(app, tmp2, (float)lineX, (float)lineY, scale,
                                    &minX, &minY, &maxX, &maxY);
                xEnd = maxX;
            }

            float selW = xEnd - xStart;
            if (selW > 0.0f) {
                float tMinX, tMinY, tMaxX, tMaxY;
                calculateTextBounds(app, " ", (float)lineX, (float)lineY, scale,
                                    &tMinX, &tMinY, &tMaxX, &tMaxY);
                prepareRectangle(app, xStart, tMinY, selW, selHeight, selColor, 0.0f);
            }
        }
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
                   app->appRenderer->currentElementBaseX,
                   app->appRenderer->cursorPosition,
                   app->appRenderer->palette->text);
    }

    // The actual drawing to the screen happens in drawFrame() which calls
    // drawBackground() and drawText() with the prepared vertices
}
