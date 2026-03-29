#include "app_state.h"
#include "unicode_search.h"
#include <provider_tags.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void clearListCurrentLayer(AppRenderer *appRenderer) {
    if (appRenderer->totalListCurrentLayer) {
        for (int i = 0; i < appRenderer->totalListCount; i++) {
            free(appRenderer->totalListCurrentLayer[i].label);
            free(appRenderer->totalListCurrentLayer[i].data);
            free(appRenderer->totalListCurrentLayer[i].navPath);
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

            // Skip meta objects unless showMetaMenu is enabled
            if (!appRenderer->showMetaMenu &&
                elem->type == FFON_OBJECT &&
                strcmp(elem->data.object->key, "meta") == 0) {
                thisId.ids[thisId.depth - 1]++;
                continue;
            }

            idArrayCopy(&appRenderer->totalListCurrentLayer[appRenderer->totalListCount].id, &thisId);

            if (elem->type == FFON_STRING) {
                // Strip <one-opt> or <many-opt> tag before other tag processing
                const char *strKey = elem->data.string;
                char *oneOptStr = NULL;
                if (providerTagHasOneOpt(strKey)) {
                    oneOptStr = providerTagStripOneOpt(strKey);
                    strKey = oneOptStr;
                } else if (providerTagHasManyOpt(strKey)) {
                    oneOptStr = strdup(strKey + MANY_OPT_TAG_LEN);
                    strKey = oneOptStr;
                }

                bool hasImage = providerTagHasImage(strKey);
                bool hasCheckboxChecked = providerTagHasCheckboxChecked(strKey);
                bool hasCheckbox = providerTagHasCheckbox(strKey);
                bool hasChecked = providerTagHasChecked(strKey);
                bool hasInputAll = providerTagHasInputAll(strKey);
                bool hasInput = providerTagHasInput(strKey);
                bool hasButton = providerTagHasButton(strKey);
                const char *prefix;
                char *stripped = NULL;

                if (hasImage) {
                    prefix = "-p";
                    stripped = providerTagStripDisplay(strKey);
                } else if (hasCheckboxChecked) {
                    prefix = "-cc";
                    stripped = providerTagExtractCheckboxCheckedContent(strKey);
                } else if (hasCheckbox) {
                    prefix = "-c";
                    stripped = providerTagExtractCheckboxContent(strKey);
                } else if (hasChecked) {
                    prefix = "-rc";
                    stripped = providerTagExtractCheckedContent(strKey);
                } else if (hasButton) {
                    prefix = "-b";
                    stripped = providerTagStripDisplay(strKey);
                } else if (hasInputAll) {
                    prefix = "-i";
                    stripped = providerTagStripDisplay(strKey);
                } else if (hasInput) {
                    prefix = "-i";
                    stripped = providerTagStripDisplay(strKey);
                } else if (parentHasRadio) {
                    prefix = "-r";
                    stripped = providerTagStripDisplay(strKey);
                } else {
                    prefix = "-";
                    stripped = providerTagStripDisplay(strKey);
                }

                char prefixed[MAX_LINE_LENGTH];
                snprintf(prefixed, sizeof(prefixed), "%s %s",
                         prefix,
                         stripped ? stripped : strKey);
                appRenderer->totalListCurrentLayer[appRenderer->totalListCount].label =
                    strdup(prefixed);
                if (hasImage) {
                    appRenderer->totalListCurrentLayer[appRenderer->totalListCount].data =
                        providerTagExtractImageContent(strKey);
                }
                free(stripped);
                free(oneOptStr);
            } else {
                // Strip <one-opt> or <many-opt> tag before other tag processing
                const char *objKey = elem->data.object->key;
                char *oneOptObj = NULL;
                if (providerTagHasOneOpt(objKey)) {
                    oneOptObj = providerTagStripOneOpt(objKey);
                    objKey = oneOptObj;
                } else if (providerTagHasManyOpt(objKey)) {
                    oneOptObj = strdup(objKey + MANY_OPT_TAG_LEN);
                    objKey = oneOptObj;
                }

                bool hasCheckboxChecked = providerTagHasCheckboxChecked(objKey);
                bool hasCheckbox = providerTagHasCheckbox(objKey);
                bool hasLink = providerTagHasLink(objKey);
                bool hasRadio = providerTagHasRadio(objKey);
                bool hasInputAll = providerTagHasInputAll(objKey);
                bool hasInput = providerTagHasInput(objKey);
                const char *prefix;
                char *stripped = NULL;

                if (hasCheckboxChecked) {
                    prefix = "+cc";
                    stripped = providerTagExtractCheckboxCheckedContent(objKey);
                } else if (hasCheckbox) {
                    prefix = "+c";
                    stripped = providerTagExtractCheckboxContent(objKey);
                } else if (hasLink) {
                    prefix = "+l";
                    stripped = providerTagStripDisplay(objKey);
                } else if (hasRadio) {
                    prefix = "+R";
                    stripped = providerTagExtractRadioContent(objKey);
                } else if (hasInputAll) {
                    prefix = "+i";
                    stripped = providerTagStripDisplay(objKey);
                } else if (hasInput) {
                    prefix = "+i";
                    stripped = providerTagStripDisplay(objKey);
                } else {
                    prefix = "+";
                }

                char prefixed[MAX_LINE_LENGTH];
                snprintf(prefixed, sizeof(prefixed), "%s %s",
                         prefix,
                         stripped ? stripped : objKey);
                appRenderer->totalListCurrentLayer[appRenderer->totalListCount].label =
                    strdup(prefixed);
                free(stripped);
                free(oneOptObj);
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
            const char *appCommands[] = {NULL};
            int numAppCommands = 0;

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
            // Strip <one-opt> or <many-opt> tag before other tag processing
            const char *strKey = elem->data.string;
            char *optStripped = NULL;
            if (providerTagHasOneOpt(strKey)) {
                optStripped = providerTagStripOneOpt(strKey);
                strKey = optStripped;
            } else if (providerTagHasManyOpt(strKey)) {
                optStripped = providerTagStripManyOpt(strKey);
                strKey = optStripped;
            }

            bool hasImage = providerTagHasImage(strKey);
            bool hasCheckboxChecked = providerTagHasCheckboxChecked(strKey);
            bool hasCheckbox = providerTagHasCheckbox(strKey);
            bool hasChecked = providerTagHasChecked(strKey);
            bool hasButton = providerTagHasButton(strKey);
            bool hasInputAll = providerTagHasInputAll(strKey);
            bool hasInput = providerTagHasInput(strKey);
            const char *prefix;
            char *stripped = NULL;

            if (hasImage) {
                prefix = "-p";
                stripped = providerTagExtractImageContent(strKey);
            } else if (hasCheckboxChecked) {
                prefix = "-cc";
                stripped = providerTagExtractCheckboxCheckedContent(strKey);
            } else if (hasCheckbox) {
                prefix = "-c";
                stripped = providerTagExtractCheckboxContent(strKey);
            } else if (hasChecked) {
                prefix = "-rc";
                stripped = providerTagExtractCheckedContent(strKey);
            } else if (hasButton) {
                prefix = "-b";
                stripped = providerTagExtractButtonDisplayText(strKey);
            } else if (hasInputAll) {
                prefix = "-i";
                stripped = providerTagExtractInputAllContent(strKey);
            } else if (hasInput) {
                prefix = "-i";
                stripped = providerTagExtractContent(strKey);
            } else {
                prefix = "-";
                stripped = providerTagStripDisplay(strKey);
            }

            char prefixed[MAX_LINE_LENGTH];
            snprintf(prefixed, sizeof(prefixed), "%s %s",
                     prefix, stripped ? stripped : strKey);
            appRenderer->totalListCurrentLayer[idx].label = strdup(prefixed);
            free(stripped);
            free(optStripped);
        } else {
            // Strip <one-opt> or <many-opt> tag before other tag processing
            const char *objKey = elem->data.object->key;
            char *optStripped = NULL;
            if (providerTagHasOneOpt(objKey)) {
                optStripped = providerTagStripOneOpt(objKey);
                objKey = optStripped;
            } else if (providerTagHasManyOpt(objKey)) {
                optStripped = providerTagStripManyOpt(objKey);
                objKey = optStripped;
            }

            bool hasCheckboxChecked = providerTagHasCheckboxChecked(objKey);
            bool hasCheckbox = providerTagHasCheckbox(objKey);
            bool hasLink = providerTagHasLink(objKey);
            bool hasRadio = providerTagHasRadio(objKey);
            bool hasInputAll = providerTagHasInputAll(objKey);
            bool hasInput = providerTagHasInput(objKey);
            const char *prefix;
            char *stripped = NULL;

            if (hasCheckboxChecked) {
                prefix = "+cc";
                stripped = providerTagExtractCheckboxCheckedContent(objKey);
            } else if (hasCheckbox) {
                prefix = "+c";
                stripped = providerTagExtractCheckboxContent(objKey);
            } else if (hasLink) {
                prefix = "+l";
                stripped = providerTagStripDisplay(objKey);
            } else if (hasRadio) {
                prefix = "+R";
                stripped = providerTagExtractRadioContent(objKey);
            } else if (hasInputAll) {
                prefix = "+i";
                stripped = providerTagExtractInputAllContent(objKey);
            } else if (hasInput) {
                prefix = "+i";
                stripped = providerTagExtractContent(objKey);
            } else {
                prefix = "+";
            }

            char prefixed[MAX_LINE_LENGTH];
            snprintf(prefixed, sizeof(prefixed), "%s %s",
                     prefix, stripped ? stripped : objKey);
            appRenderer->totalListCurrentLayer[idx].label = strdup(prefixed);
            free(stripped);
            free(optStripped);
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

    // If the active provider supports deep search, use it instead of FFON-tree traversal
    Provider *provider = providerGetActive(appRenderer);
    if (provider && provider->collectDeepSearchItems) {
        int deepCount = 0;
        SearchResultItem *items = provider->collectDeepSearchItems(provider, &deepCount);
        if (items && deepCount > 0) {
            appRenderer->totalListCurrentLayer = calloc(deepCount, sizeof(ListItem));
            if (!appRenderer->totalListCurrentLayer) {
                for (int i = 0; i < deepCount; i++) {
                    free(items[i].label); free(items[i].breadcrumb); free(items[i].navPath);
                }
                free(items);
                return;
            }
            int rootIdx = appRenderer->currentId.ids[0];
            for (int i = 0; i < deepCount; i++) {
                appRenderer->totalListCurrentLayer[i].id.depth = 1;
                appRenderer->totalListCurrentLayer[i].id.ids[0] = rootIdx;
                // Transfer string ownership into ListItem (no extra strdup)
                appRenderer->totalListCurrentLayer[i].label    = items[i].label;
                appRenderer->totalListCurrentLayer[i].data     = items[i].breadcrumb;
                appRenderer->totalListCurrentLayer[i].navPath  = items[i].navPath;
                appRenderer->totalListCount++;
            }
            free(items); // free array only; strings now owned by ListItem
            return;
        }
        if (items) {
            for (int i = 0; i < deepCount; i++) {
                free(items[i].label); free(items[i].breadcrumb); free(items[i].navPath);
            }
            free(items);
        }
    }

    // Fallback: walk in-memory FFON tree
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
        const char *curLabel = appRenderer->totalListCurrentLayer[i].label;
        bool matches;
        if (appRenderer->totalListCurrentLayer[i].navPath) {
            // Deep filesystem item: prefix-match on bare filename (skip "- " or "+ " prefix)
            const char *bareName = (curLabel[0] != '\0' && curLabel[1] == ' ') ? curLabel + 2 : curLabel;
            const char *found = utf8_stristr(bareName, searchString);
            matches = (found == bareName);
        } else {
            matches = (utf8_stristr(curLabel, searchString) != NULL);
        }
        if (matches) {
            idArrayCopy(&appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].id,
                         &appRenderer->totalListCurrentLayer[i].id);
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].label =
                appRenderer->totalListCurrentLayer[i].label; // Share pointer
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].data =
                appRenderer->totalListCurrentLayer[i].data; // Share pointer
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].navPath =
                appRenderer->totalListCurrentLayer[i].navPath; // Share pointer
            appRenderer->filteredListCount++;
        }
    }

    // Reset list index
    if (appRenderer->listIndex >= appRenderer->filteredListCount) {
        appRenderer->listIndex = appRenderer->filteredListCount > 0 ? appRenderer->filteredListCount - 1 : 0;
    }
}
