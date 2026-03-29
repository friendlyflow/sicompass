#include <win_compat.h>
#include <webbrowser.h>
#include <webbrowser_provider.h>
#include <provider_tags.h>
#include <ffon.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

// Cached page data
typedef struct {
    char url[4096];
    FfonElement **elements;
    int elementCount;
} CachedPage;

static CachedPage g_cachedPage = {0};
static char g_currentUrl[4096] = "";
static Provider *g_provider = NULL;

static void wbClearCache(void) {
    if (g_cachedPage.elements) {
        for (int i = 0; i < g_cachedPage.elementCount; i++)
            ffonElementDestroy(g_cachedPage.elements[i]);
        free(g_cachedPage.elements);
        g_cachedPage.elements = NULL;
        g_cachedPage.elementCount = 0;
    }
    g_cachedPage.url[0] = '\0';
}

static FfonElement** wbFetch(const char *path, int *outCount) {
    (void)path;

    // URL bar
    char urlBuf[4200];
    if (g_currentUrl[0] == '\0')
        snprintf(urlBuf, sizeof(urlBuf), "<input>https://</input>");
    else
        snprintf(urlBuf, sizeof(urlBuf), "<input>%s</input>", g_currentUrl);

    FfonElement **elems = malloc(sizeof(FfonElement*));

    if (!g_cachedPage.elements) {
        // No page content: return as string so it can't be navigated into
        elems[0] = ffonElementCreateString(urlBuf);
    } else {
        // Page loaded: return as object with page content as children
        elems[0] = ffonElementCreateObject(urlBuf);
        for (int i = 0; i < g_cachedPage.elementCount; i++)
            ffonObjectAddElement(elems[0]->data.object, ffonElementClone(g_cachedPage.elements[i]));
    }

    // Prepend meta
    FfonElement *meta = ffonElementCreateObject("meta");
    if (meta) {
        ffonObjectAddElement(meta->data.object, ffonElementCreateString("I       Edit URL"));
        ffonObjectAddElement(meta->data.object, ffonElementCreateString("/       Search"));
        ffonObjectAddElement(meta->data.object, ffonElementCreateString("F5      Refresh"));
        ffonObjectAddElement(meta->data.object, ffonElementCreateString(":       Commands"));
        FfonElement **result = malloc(2 * sizeof(FfonElement*));
        if (result) {
            result[0] = meta;
            result[1] = elems[0];
            free(elems);
            *outCount = 2;
            return result;
        }
        ffonElementDestroy(meta);
    }
    *outCount = 1;
    return elems;
}

// Fetch and parse the current URL into the cache. Returns true on success.
static bool wbFetchPage(void) {
    if (g_currentUrl[0] == '\0') return false;
    wbClearCache();

    char *html = webbrowserFetchUrl(g_currentUrl);
    if (!html) {
        snprintf(g_provider->errorMessage, sizeof(g_provider->errorMessage),
                 "failed to fetch URL");
        return false;
    }
    int count = 0;
    FfonElement **parsed = webbrowserHtmlToFfon(html, g_currentUrl, &count);
    free(html);
    if (parsed && count > 0) {
        strncpy(g_cachedPage.url, g_currentUrl, sizeof(g_cachedPage.url) - 1);
        g_cachedPage.url[sizeof(g_cachedPage.url) - 1] = '\0';
        g_cachedPage.elements = parsed;
        g_cachedPage.elementCount = count;
        return true;
    }
    free(parsed);
    snprintf(g_provider->errorMessage, sizeof(g_provider->errorMessage),
             "failed to fetch URL");
    return false;
}

static bool wbCommit(const char *path, const char *oldName, const char *newName) {
    (void)oldName;
    (void)path;
    if (newName && newName[0]) {
        strncpy(g_currentUrl, newName, sizeof(g_currentUrl) - 1);
        g_currentUrl[sizeof(g_currentUrl) - 1] = '\0';
        return wbFetchPage();
    }
    return true;
}

static const char *wb_commands[] = { "refresh" };

static const char** wbGetCommands(int *outCount) {
    *outCount = 1;
    return wb_commands;
}

static FfonElement* wbHandleCommand(const char *path, const char *command,
                                     const char *elementKey, int elementType,
                                     char *errorMsg, int errorMsgSize) {
    (void)path;
    (void)elementKey;
    (void)elementType;
    (void)errorMsg;
    (void)errorMsgSize;

    if (strcmp(command, "refresh") == 0) {
        wbFetchPage();
        return NULL;
    }

    if (errorMsg && errorMsgSize > 0)
        snprintf(errorMsg, errorMsgSize, "unknown command: %s", command);
    return NULL;
}

static bool wbExecuteCommand(const char *path, const char *command,
                              const char *selection) {
    (void)path;
    (void)command;
    (void)selection;
    return true;
}

Provider* webbrowserGetProvider(void) {
    if (!g_provider) {
        static ProviderOps ops = {
            .name = "webbrowser",
            .displayName = "web browser",
            .fetch = wbFetch,
            .commit = wbCommit,
            .createDirectory = NULL,
            .createFile = NULL,
            .deleteItem = NULL,
            .copyItem = NULL,
            .getCommands = wbGetCommands,
            .handleCommand = wbHandleCommand,
            .getCommandListItems = NULL,
            .executeCommand = wbExecuteCommand,
            .collectDeepSearchItems = NULL,
        };
        g_provider = providerCreate(&ops);
    }
    return g_provider;
}

GCC_CONSTRUCTOR(webbrowserRegisterFactory) {
    providerFactoryRegister("web browser", webbrowserGetProvider);
}
