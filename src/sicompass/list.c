#include "view.h"
#include "provider.h"
#include "unicode_search.h"
#include <provider_tags.h>
#include <stdlib.h>
#include <string.h>

void clearListCurrentLayer(AppRenderer *appRenderer) {
    if (appRenderer->totalListCurrentLayer) {
        for (int i = 0; i < appRenderer->totalListCount; i++) {
            free(appRenderer->totalListCurrentLayer[i].label);
            free(appRenderer->totalListCurrentLayer[i].data);
        }
        free(appRenderer->totalListCurrentLayer);
        appRenderer->totalListCurrentLayer = NULL;
        appRenderer->totalListCount = 0;
    }

    if (appRenderer->filteredListCurrentLayer) {
        // Don't free values, they're shared with totalListCurrentLayer
        free(appRenderer->filteredListCurrentLayer);
        appRenderer->filteredListCurrentLayer = NULL;
        appRenderer->filteredListCount = 0;
    }

    appRenderer->listIndex = 0;
}

void createListCurrentLayer(AppRenderer *appRenderer) {
    clearListCurrentLayer(appRenderer);
    appRenderer->errorMessage[0] = '\0';

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT
    ) {
        // List all elements in current layer
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (!arr) return;

        appRenderer->totalListCurrentLayer = calloc(count, sizeof(ListItem));
        if (!appRenderer->totalListCurrentLayer) return;

        IdArray thisId;
        idArrayCopy(&thisId, &appRenderer->currentId);
        thisId.ids[thisId.depth - 1] = 0;

        // Detect if parent has <radio> tag (for -r prefix on children)
        bool parentHasRadio = false;
        IdArray parentId;
        if (appRenderer->currentId.depth >= 2) {
            idArrayCopy(&parentId, &appRenderer->currentId);
            idArrayPop(&parentId);
            int parentCount;
            FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &parentId, &parentCount);
            if (parentArr) {
                int parentIdx = parentId.ids[parentId.depth - 1];
                if (parentIdx >= 0 && parentIdx < parentCount &&
                    parentArr[parentIdx]->type == FFON_OBJECT) {
                    parentHasRadio = providerTagHasRadio(parentArr[parentIdx]->data.object->key);
                }
            }
        }

        for (int i = 0; i < count; i++) {
            FfonElement *elem = arr[i];

            idArrayCopy(&appRenderer->totalListCurrentLayer[appRenderer->totalListCount].id, &thisId);

            if (elem->type == FFON_STRING) {
                bool hasImage = providerTagHasImage(elem->data.string);
                bool hasCheckboxChecked = providerTagHasCheckboxChecked(elem->data.string);
                bool hasCheckbox = providerTagHasCheckbox(elem->data.string);
                bool hasChecked = providerTagHasChecked(elem->data.string);
                bool hasInput = providerTagHasInput(elem->data.string);
                const char *prefix;
                char *stripped = NULL;

                if (hasImage) {
                    prefix = "-p";
                    stripped = providerTagExtractImageContent(elem->data.string);
                } else if (hasCheckboxChecked) {
                    prefix = "-cc";
                    stripped = providerTagExtractCheckboxCheckedContent(elem->data.string);
                } else if (hasCheckbox) {
                    prefix = "-c";
                    stripped = providerTagExtractCheckboxContent(elem->data.string);
                } else if (hasChecked) {
                    prefix = "-rc";
                    stripped = providerTagExtractCheckedContent(elem->data.string);
                } else if (hasInput) {
                    prefix = "-i";
                    stripped = providerTagStripDisplay(elem->data.string);
                } else if (parentHasRadio) {
                    prefix = "-r";
                } else {
                    prefix = "-";
                }

                char prefixed[MAX_LINE_LENGTH];
                snprintf(prefixed, sizeof(prefixed), "%s %s",
                         prefix,
                         stripped ? stripped : elem->data.string);
                appRenderer->totalListCurrentLayer[appRenderer->totalListCount].label =
                    strdup(prefixed);
                free(stripped);
            } else {
                bool hasLink = providerTagHasLink(elem->data.object->key);
                bool hasRadio = providerTagHasRadio(elem->data.object->key);
                bool hasInput = providerTagHasInput(elem->data.object->key);
                const char *prefix;
                char *stripped = NULL;

                if (hasLink) {
                    prefix = "+l";
                    stripped = providerTagExtractLinkContent(elem->data.object->key);
                } else if (hasRadio) {
                    prefix = "+R";
                    stripped = providerTagExtractRadioContent(elem->data.object->key);
                } else if (hasInput) {
                    prefix = "+i";
                    stripped = providerTagStripDisplay(elem->data.object->key);
                } else {
                    prefix = "+";
                }

                char prefixed[MAX_LINE_LENGTH];
                snprintf(prefixed, sizeof(prefixed), "%s %s",
                         prefix,
                         stripped ? stripped : elem->data.object->key);
                appRenderer->totalListCurrentLayer[appRenderer->totalListCount].label =
                    strdup(prefixed);
                free(stripped);
            }

            appRenderer->totalListCount++;
            thisId.ids[thisId.depth - 1]++;
        }

    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        if (appRenderer->currentCommand == COMMAND_PROVIDER) {
            // Provider command needs secondary selection (e.g., applications for open-with)
            int ecount;
            FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &appRenderer->currentId, &ecount);
            if (earr && ecount > 0) {
                int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (eidx >= 0 && eidx < ecount) {
                    const char *elementKey = (earr[eidx]->type == FFON_STRING) ?
                        earr[eidx]->data.string : earr[eidx]->data.object->key;

                    int itemCount = 0;
                    ProviderListItem *items = providerGetCommandListItems(appRenderer,
                        appRenderer->providerCommandName, &itemCount);
                    if (items && itemCount > 0) {
                        appRenderer->totalListCurrentLayer = calloc(itemCount, sizeof(ListItem));
                        if (!appRenderer->totalListCurrentLayer) {
                            providerFreeCommandListItems(items, itemCount);
                            return;
                        }
                        for (int i = 0; i < itemCount; i++) {
                            appRenderer->totalListCurrentLayer[i].id.depth = 1;
                            appRenderer->totalListCurrentLayer[i].id.ids[0] = i;
                            appRenderer->totalListCurrentLayer[i].label = strdup(items[i].label);
                            appRenderer->totalListCurrentLayer[i].data = items[i].data ? strdup(items[i].data) : NULL;
                            appRenderer->totalListCount++;
                        }
                        providerFreeCommandListItems(items, itemCount);
                    }
                }
            }
        } else {
            // List available commands: app commands + provider commands
            const char *appCommands[] = {"editor mode", "operator mode"};
            int numAppCommands = 2;

            // Get provider commands for current element
            int numProviderCommands = 0;
            const char **providerCmds = NULL;
            int ecount;
            FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &appRenderer->currentId, &ecount);
            if (earr && ecount > 0) {
                int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (eidx >= 0 && eidx < ecount) {
                    const char *elementKey = (earr[eidx]->type == FFON_STRING) ?
                        earr[eidx]->data.string : earr[eidx]->data.object->key;
                    providerCmds = providerGetCommands(appRenderer, &numProviderCommands);
                }
            }

            int numCommands = numAppCommands + numProviderCommands;
            appRenderer->totalListCurrentLayer = calloc(numCommands, sizeof(ListItem));
            if (!appRenderer->totalListCurrentLayer) return;

            int idx = 0;
            for (int i = 0; i < numAppCommands; i++) {
                appRenderer->totalListCurrentLayer[idx].id.depth = 1;
                appRenderer->totalListCurrentLayer[idx].id.ids[0] = idx;
                appRenderer->totalListCurrentLayer[idx].label = strdup(appCommands[i]);
                appRenderer->totalListCount++;
                idx++;
            }
            for (int i = 0; i < numProviderCommands; i++) {
                appRenderer->totalListCurrentLayer[idx].id.depth = 1;
                appRenderer->totalListCurrentLayer[idx].id.ids[0] = idx;
                appRenderer->totalListCurrentLayer[idx].label = strdup(providerCmds[i]);
                appRenderer->totalListCount++;
                idx++;
            }
        }
    }
}

static int countElementsRecursive(FfonElement **elements, int count, int depth) {
    if (depth >= MAX_ID_DEPTH) return 0;
    int total = count;
    for (int i = 0; i < count; i++) {
        if (elements[i]->type == FFON_OBJECT) {
            total += countElementsRecursive(
                elements[i]->data.object->elements,
                elements[i]->data.object->count,
                depth + 1);
        }
    }
    return total;
}

static void collectItemsRecursive(AppRenderer *appRenderer, FfonElement **elements, int count,
                                   IdArray *basePath, const char *breadcrumb) {
    if (basePath->depth >= MAX_ID_DEPTH) return;

    IdArray itemId;
    idArrayCopy(&itemId, basePath);
    itemId.ids[itemId.depth - 1] = 0;

    for (int i = 0; i < count; i++) {
        FfonElement *elem = elements[i];
        int idx = appRenderer->totalListCount;

        idArrayCopy(&appRenderer->totalListCurrentLayer[idx].id, &itemId);

        if (elem->type == FFON_STRING) {
            bool hasImage = providerTagHasImage(elem->data.string);
            bool hasCheckboxChecked = providerTagHasCheckboxChecked(elem->data.string);
            bool hasCheckbox = providerTagHasCheckbox(elem->data.string);
            bool hasChecked = providerTagHasChecked(elem->data.string);
            bool hasInput = providerTagHasInput(elem->data.string);
            const char *prefix;
            char *stripped = NULL;

            if (hasImage) {
                prefix = "-p";
                stripped = providerTagExtractImageContent(elem->data.string);
            } else if (hasCheckboxChecked) {
                prefix = "-cc";
                stripped = providerTagExtractCheckboxCheckedContent(elem->data.string);
            } else if (hasCheckbox) {
                prefix = "-c";
                stripped = providerTagExtractCheckboxContent(elem->data.string);
            } else if (hasChecked) {
                prefix = "-rc";
                stripped = providerTagExtractCheckedContent(elem->data.string);
            } else if (hasInput) {
                prefix = "-i";
                stripped = providerTagExtractContent(elem->data.string);
            } else {
                prefix = "-";
            }

            char prefixed[MAX_LINE_LENGTH];
            snprintf(prefixed, sizeof(prefixed), "%s %s",
                     prefix, stripped ? stripped : elem->data.string);
            appRenderer->totalListCurrentLayer[idx].label = strdup(prefixed);
            free(stripped);
        } else {
            bool hasLink = providerTagHasLink(elem->data.object->key);
            bool hasRadio = providerTagHasRadio(elem->data.object->key);
            bool hasInput = providerTagHasInput(elem->data.object->key);
            const char *prefix;
            char *stripped = NULL;

            if (hasLink) {
                prefix = "+l";
                stripped = providerTagExtractLinkContent(elem->data.object->key);
            } else if (hasRadio) {
                prefix = "+R";
                stripped = providerTagExtractRadioContent(elem->data.object->key);
            } else if (hasInput) {
                prefix = "+i";
                stripped = providerTagExtractContent(elem->data.object->key);
            } else {
                prefix = "+";
            }

            char prefixed[MAX_LINE_LENGTH];
            snprintf(prefixed, sizeof(prefixed), "%s %s",
                     prefix, stripped ? stripped : elem->data.object->key);
            appRenderer->totalListCurrentLayer[idx].label = strdup(prefixed);
            free(stripped);
        }

        // Store breadcrumb in data field
        appRenderer->totalListCurrentLayer[idx].data =
            (breadcrumb && breadcrumb[0] != '\0') ? strdup(breadcrumb) : NULL;

        appRenderer->totalListCount++;
        itemId.ids[itemId.depth - 1]++;

        // Recurse into object children
        if (elem->type == FFON_OBJECT && elem->data.object->count > 0) {
            char *displayName = providerTagStripDisplay(elem->data.object->key);
            char newBreadcrumb[MAX_LINE_LENGTH];
            if (breadcrumb && breadcrumb[0] != '\0') {
                snprintf(newBreadcrumb, sizeof(newBreadcrumb), "%s%s > ", breadcrumb, displayName);
            } else {
                snprintf(newBreadcrumb, sizeof(newBreadcrumb), "%s > ", displayName);
            }
            free(displayName);

            IdArray childPath;
            idArrayCopy(&childPath, &itemId);
            childPath.ids[childPath.depth - 1] = i;
            idArrayPush(&childPath, 0);

            collectItemsRecursive(appRenderer, elem->data.object->elements,
                                  elem->data.object->count, &childPath, newBreadcrumb);
        }
    }
}

void createListExtendedSearch(AppRenderer *appRenderer) {
    clearListCurrentLayer(appRenderer);
    appRenderer->errorMessage[0] = '\0';

    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                     &appRenderer->currentId, &count);
    if (!arr) return;

    int totalCount = countElementsRecursive(arr, count, appRenderer->currentId.depth);
    if (totalCount == 0) return;

    appRenderer->totalListCurrentLayer = calloc(totalCount, sizeof(ListItem));
    if (!appRenderer->totalListCurrentLayer) return;

    IdArray basePath;
    idArrayCopy(&basePath, &appRenderer->currentId);
    basePath.ids[basePath.depth - 1] = 0;

    collectItemsRecursive(appRenderer, arr, count, &basePath, "");
}

void populateListCurrentLayer(AppRenderer *appRenderer, const char *searchString) {
    if (!searchString || strlen(searchString) == 0) {
        // No search, use all items
        if (appRenderer->filteredListCurrentLayer) {
            free(appRenderer->filteredListCurrentLayer);
        }
        appRenderer->filteredListCurrentLayer = NULL;
        appRenderer->filteredListCount = 0;
        appRenderer->listIndex = 0;
        return;
    }

    // Simple substring search
    if (appRenderer->filteredListCurrentLayer) {
        free(appRenderer->filteredListCurrentLayer);
    }

    appRenderer->filteredListCurrentLayer = calloc(appRenderer->totalListCount, sizeof(ListItem));
    if (!appRenderer->filteredListCurrentLayer) return;

    appRenderer->filteredListCount = 0;

    for (int i = 0; i < appRenderer->totalListCount; i++) {
        if (utf8_stristr(appRenderer->totalListCurrentLayer[i].label, searchString) != NULL) {
            idArrayCopy(&appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].id,
                         &appRenderer->totalListCurrentLayer[i].id);
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].label =
                appRenderer->totalListCurrentLayer[i].label; // Share pointer
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].data =
                appRenderer->totalListCurrentLayer[i].data; // Share pointer
            appRenderer->filteredListCount++;
        }
    }

    // Reset list index
    if (appRenderer->listIndex >= appRenderer->filteredListCount) {
        appRenderer->listIndex = appRenderer->filteredListCount > 0 ? appRenderer->filteredListCount - 1 : 0;
    }
}
