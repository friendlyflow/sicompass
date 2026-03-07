#include "webbrowser.h"
#include <curl/curl.h>
#include <lexbor/html/html.h>
#include <lexbor/dom/dom.h>
#include <lexbor/tag/tag.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <ctype.h>

// --- curl fetch ---

typedef struct {
    char *data;
    size_t size;
    size_t capacity;
} ResponseBuffer;

static size_t curlWriteCallback(char *ptr, size_t size, size_t nmemb, void *userdata) {
    ResponseBuffer *buf = (ResponseBuffer *)userdata;
    size_t total = size * nmemb;
    if (buf->size + total >= buf->capacity) {
        size_t newCap = (buf->capacity == 0) ? 4096 : buf->capacity * 2;
        while (newCap < buf->size + total + 1) newCap *= 2;
        char *newData = realloc(buf->data, newCap);
        if (!newData) return 0;
        buf->data = newData;
        buf->capacity = newCap;
    }
    memcpy(buf->data + buf->size, ptr, total);
    buf->size += total;
    buf->data[buf->size] = '\0';
    return total;
}

char* webbrowserFetchUrl(const char *url) {
    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    ResponseBuffer buf = {NULL, 0, 0};

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);
    curl_easy_setopt(curl, CURLOPT_USERAGENT, "sicompass/1.0");

    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        free(buf.data);
        return NULL;
    }

    return buf.data;
}

// --- HTML to FFON conversion ---

#define MAX_TEXT_BUF 8192
#define MAX_ELEMENTS 4096
#define MAX_HEADING_STACK 6

typedef struct {
    FfonElement *element;
    int level;
} HeadingEntry;

typedef struct {
    FfonElement **result;
    int resultCount;
    int resultCapacity;
    HeadingEntry stack[MAX_HEADING_STACK];
    int stackDepth;
    const char *baseUrl;
} ParseContext;

static void ctxAddToParent(ParseContext *ctx, FfonElement *elem) {
    if (ctx->stackDepth > 0) {
        ffonObjectAddElement(ctx->stack[ctx->stackDepth - 1].element->data.object, elem);
    } else {
        if (ctx->resultCount >= ctx->resultCapacity) {
            ctx->resultCapacity = ctx->resultCapacity ? ctx->resultCapacity * 2 : 64;
            ctx->result = realloc(ctx->result, ctx->resultCapacity * sizeof(FfonElement*));
        }
        ctx->result[ctx->resultCount++] = elem;
    }
}

static int headingLevel(lxb_tag_id_t tag) {
    switch (tag) {
        case LXB_TAG_H1: return 1;
        case LXB_TAG_H2: return 2;
        case LXB_TAG_H3: return 3;
        case LXB_TAG_H4: return 4;
        case LXB_TAG_H5: return 5;
        case LXB_TAG_H6: return 6;
        default: return 0;
    }
}

static bool isSkippedTag(lxb_tag_id_t tag) {
    return tag == LXB_TAG_SCRIPT || tag == LXB_TAG_STYLE ||
           tag == LXB_TAG_NOSCRIPT || tag == LXB_TAG_SVG ||
           tag == LXB_TAG_HEAD || tag == LXB_TAG_NAV ||
           tag == LXB_TAG_FOOTER;
}

static bool isBlockContainer(lxb_tag_id_t tag) {
    return tag == LXB_TAG_BODY || tag == LXB_TAG_DIV ||
           tag == LXB_TAG_SECTION || tag == LXB_TAG_ARTICLE ||
           tag == LXB_TAG_MAIN || tag == LXB_TAG_HEADER ||
           tag == LXB_TAG_ASIDE || tag == LXB_TAG_FIGURE;
}

// Extract all text from a node, flattening inline elements.
// For <a> tags, inserts <link>href</link> markup.
static void extractText(lxb_dom_node_t *node, char *buf, size_t bufSize, size_t *pos) {
    for (lxb_dom_node_t *child = lxb_dom_node_first_child(node);
         child != NULL;
         child = lxb_dom_node_next(child))
    {
        if (child->type == LXB_DOM_NODE_TYPE_TEXT) {
            size_t len = 0;
            const lxb_char_t *text = lxb_dom_node_text_content(child, &len);
            if (text && len > 0 && *pos < bufSize - 1) {
                size_t copyLen = (len < bufSize - 1 - *pos) ? len : bufSize - 1 - *pos;
                memcpy(buf + *pos, text, copyLen);
                *pos += copyLen;
                buf[*pos] = '\0';
            }
        } else if (child->type == LXB_DOM_NODE_TYPE_ELEMENT) {
            lxb_dom_element_t *el = lxb_dom_interface_element(child);
            lxb_tag_id_t tag = lxb_dom_element_tag_id(el);

            if (tag == LXB_TAG_BR) {
                if (*pos < bufSize - 2) {
                    buf[*pos] = '\n';
                    (*pos)++;
                    buf[*pos] = '\0';
                }
                continue;
            }

            if (tag == LXB_TAG_A) {
                size_t hrefLen = 0;
                const lxb_char_t *href = lxb_dom_element_get_attribute(
                    el, (const lxb_char_t *)"href", 4, &hrefLen);
                if (href && hrefLen > 0 && *pos < bufSize - 20) {
                    int written = snprintf(buf + *pos, bufSize - *pos,
                                           "<link>%.*s</link>", (int)hrefLen, href);
                    if (written > 0) *pos += written;
                }
            }

            extractText(child, buf, bufSize, pos);
        }
    }
}

static void getNodeText(lxb_dom_node_t *node, char *buf, size_t bufSize) {
    size_t pos = 0;
    buf[0] = '\0';
    extractText(node, buf, bufSize, &pos);

    // Trim leading/trailing whitespace
    size_t start = 0;
    while (start < pos && isspace((unsigned char)buf[start])) start++;
    size_t end = pos;
    while (end > start && isspace((unsigned char)buf[end - 1])) end--;

    if (start > 0 || end < pos) {
        size_t newLen = end - start;
        memmove(buf, buf + start, newLen);
        buf[newLen] = '\0';
    }
}

static void processNode(lxb_dom_node_t *node, ParseContext *ctx);

static void processChildren(lxb_dom_node_t *node, ParseContext *ctx) {
    for (lxb_dom_node_t *child = lxb_dom_node_first_child(node);
         child != NULL;
         child = lxb_dom_node_next(child))
    {
        if (child->type == LXB_DOM_NODE_TYPE_ELEMENT) {
            processNode(child, ctx);
        }
    }
}

static void processNode(lxb_dom_node_t *node, ParseContext *ctx) {
    lxb_dom_element_t *el = lxb_dom_interface_element(node);
    lxb_tag_id_t tag = lxb_dom_element_tag_id(el);

    if (isSkippedTag(tag)) return;

    int hlevel = headingLevel(tag);
    if (hlevel > 0) {
        // Pop stack entries with level >= this heading
        while (ctx->stackDepth > 0 &&
               ctx->stack[ctx->stackDepth - 1].level >= hlevel) {
            ctx->stackDepth--;
        }

        char textBuf[MAX_TEXT_BUF];
        getNodeText(node, textBuf, sizeof(textBuf));
        if (textBuf[0] == '\0') return;

        FfonElement *obj = ffonElementCreateObject(textBuf);
        ctxAddToParent(ctx, obj);

        if (ctx->stackDepth < MAX_HEADING_STACK) {
            ctx->stack[ctx->stackDepth].element = obj;
            ctx->stack[ctx->stackDepth].level = hlevel;
            ctx->stackDepth++;
        }
        return;
    }

    if (tag == LXB_TAG_P || tag == LXB_TAG_BLOCKQUOTE || tag == LXB_TAG_PRE) {
        char textBuf[MAX_TEXT_BUF];
        getNodeText(node, textBuf, sizeof(textBuf));
        if (textBuf[0] != '\0') {
            ctxAddToParent(ctx, ffonElementCreateString(textBuf));
        }
        return;
    }

    if (tag == LXB_TAG_UL || tag == LXB_TAG_OL) {
        const char *listName = (tag == LXB_TAG_OL) ? "ordered list" : "list";
        FfonElement *listObj = ffonElementCreateObject(listName);

        int itemNum = 0;
        for (lxb_dom_node_t *child = lxb_dom_node_first_child(node);
             child != NULL;
             child = lxb_dom_node_next(child))
        {
            if (child->type != LXB_DOM_NODE_TYPE_ELEMENT) continue;
            lxb_dom_element_t *childEl = lxb_dom_interface_element(child);
            if (lxb_dom_element_tag_id(childEl) != LXB_TAG_LI) continue;

            char textBuf[MAX_TEXT_BUF];
            getNodeText(child, textBuf, sizeof(textBuf));
            if (textBuf[0] == '\0') continue;

            itemNum++;
            if (tag == LXB_TAG_OL) {
                char numbered[MAX_TEXT_BUF + 16];
                snprintf(numbered, sizeof(numbered), "%d. %s", itemNum, textBuf);
                ffonObjectAddElement(listObj->data.object,
                                     ffonElementCreateString(numbered));
            } else {
                ffonObjectAddElement(listObj->data.object,
                                     ffonElementCreateString(textBuf));
            }
        }

        if (listObj->data.object->count > 0) {
            ctxAddToParent(ctx, listObj);
        } else {
            ffonElementDestroy(listObj);
        }
        return;
    }

    if (tag == LXB_TAG_TABLE) {
        FfonElement *tableObj = ffonElementCreateObject("table");

        for (lxb_dom_node_t *child = lxb_dom_node_first_child(node);
             child != NULL;
             child = lxb_dom_node_next(child))
        {
            if (child->type != LXB_DOM_NODE_TYPE_ELEMENT) continue;
            lxb_dom_element_t *childEl = lxb_dom_interface_element(child);
            lxb_tag_id_t childTag = lxb_dom_element_tag_id(childEl);

            // Handle direct <tr> or <thead>/<tbody>/<tfoot> wrappers
            if (childTag == LXB_TAG_TR) {
                char rowBuf[MAX_TEXT_BUF];
                rowBuf[0] = '\0';
                size_t rowPos = 0;
                bool firstCell = true;

                for (lxb_dom_node_t *cell = lxb_dom_node_first_child(child);
                     cell != NULL;
                     cell = lxb_dom_node_next(cell))
                {
                    if (cell->type != LXB_DOM_NODE_TYPE_ELEMENT) continue;
                    lxb_dom_element_t *cellEl = lxb_dom_interface_element(cell);
                    lxb_tag_id_t cellTag = lxb_dom_element_tag_id(cellEl);
                    if (cellTag != LXB_TAG_TD && cellTag != LXB_TAG_TH) continue;

                    char cellBuf[1024];
                    getNodeText(cell, cellBuf, sizeof(cellBuf));
                    if (!firstCell && rowPos < sizeof(rowBuf) - 4) {
                        rowPos += snprintf(rowBuf + rowPos, sizeof(rowBuf) - rowPos, " | ");
                    }
                    if (rowPos < sizeof(rowBuf) - 1) {
                        rowPos += snprintf(rowBuf + rowPos, sizeof(rowBuf) - rowPos, "%s", cellBuf);
                    }
                    firstCell = false;
                }

                if (rowBuf[0] != '\0') {
                    ffonObjectAddElement(tableObj->data.object,
                                         ffonElementCreateString(rowBuf));
                }
            } else if (childTag == LXB_TAG_THEAD || childTag == LXB_TAG_TBODY ||
                       childTag == LXB_TAG_TFOOT) {
                // Recurse into thead/tbody/tfoot to find <tr>s
                for (lxb_dom_node_t *tr = lxb_dom_node_first_child(child);
                     tr != NULL;
                     tr = lxb_dom_node_next(tr))
                {
                    if (tr->type != LXB_DOM_NODE_TYPE_ELEMENT) continue;
                    lxb_dom_element_t *trEl = lxb_dom_interface_element(tr);
                    if (lxb_dom_element_tag_id(trEl) != LXB_TAG_TR) continue;

                    char rowBuf[MAX_TEXT_BUF];
                    rowBuf[0] = '\0';
                    size_t rowPos = 0;
                    bool firstCell = true;

                    for (lxb_dom_node_t *cell = lxb_dom_node_first_child(tr);
                         cell != NULL;
                         cell = lxb_dom_node_next(cell))
                    {
                        if (cell->type != LXB_DOM_NODE_TYPE_ELEMENT) continue;
                        lxb_dom_element_t *cellEl = lxb_dom_interface_element(cell);
                        lxb_tag_id_t cellTag = lxb_dom_element_tag_id(cellEl);
                        if (cellTag != LXB_TAG_TD && cellTag != LXB_TAG_TH) continue;

                        char cellBuf[1024];
                        getNodeText(cell, cellBuf, sizeof(cellBuf));
                        if (!firstCell && rowPos < sizeof(rowBuf) - 4) {
                            rowPos += snprintf(rowBuf + rowPos, sizeof(rowBuf) - rowPos, " | ");
                        }
                        if (rowPos < sizeof(rowBuf) - 1) {
                            rowPos += snprintf(rowBuf + rowPos, sizeof(rowBuf) - rowPos, "%s", cellBuf);
                        }
                        firstCell = false;
                    }

                    if (rowBuf[0] != '\0') {
                        ffonObjectAddElement(tableObj->data.object,
                                             ffonElementCreateString(rowBuf));
                    }
                }
            }
        }

        if (tableObj->data.object->count > 0) {
            ctxAddToParent(ctx, tableObj);
        } else {
            ffonElementDestroy(tableObj);
        }
        return;
    }

    if (tag == LXB_TAG_DL) {
        FfonElement *dlObj = ffonElementCreateObject("definition list");
        FfonElement *currentDt = NULL;

        for (lxb_dom_node_t *child = lxb_dom_node_first_child(node);
             child != NULL;
             child = lxb_dom_node_next(child))
        {
            if (child->type != LXB_DOM_NODE_TYPE_ELEMENT) continue;
            lxb_dom_element_t *childEl = lxb_dom_interface_element(child);
            lxb_tag_id_t childTag = lxb_dom_element_tag_id(childEl);

            char textBuf[MAX_TEXT_BUF];
            getNodeText(child, textBuf, sizeof(textBuf));
            if (textBuf[0] == '\0') continue;

            if (childTag == LXB_TAG_DT) {
                currentDt = ffonElementCreateObject(textBuf);
                ffonObjectAddElement(dlObj->data.object, currentDt);
            } else if (childTag == LXB_TAG_DD) {
                if (currentDt) {
                    ffonObjectAddElement(currentDt->data.object,
                                         ffonElementCreateString(textBuf));
                } else {
                    ffonObjectAddElement(dlObj->data.object,
                                         ffonElementCreateString(textBuf));
                }
            }
        }

        if (dlObj->data.object->count > 0) {
            ctxAddToParent(ctx, dlObj);
        } else {
            ffonElementDestroy(dlObj);
        }
        return;
    }

    if (tag == LXB_TAG_IMG) {
        size_t altLen = 0;
        const lxb_char_t *alt = lxb_dom_element_get_attribute(
            el, (const lxb_char_t *)"alt", 3, &altLen);
        if (alt && altLen > 0) {
            char imgBuf[MAX_TEXT_BUF];
            snprintf(imgBuf, sizeof(imgBuf), "[image: %.*s]", (int)altLen, alt);
            ctxAddToParent(ctx, ffonElementCreateString(imgBuf));
        }
        return;
    }

    // Block containers: recurse into children
    if (isBlockContainer(tag)) {
        processChildren(node, ctx);
        return;
    }

    // Standalone <a> at block level (not inline within a <p>)
    if (tag == LXB_TAG_A) {
        char textBuf[MAX_TEXT_BUF];
        getNodeText(node, textBuf, sizeof(textBuf));
        if (textBuf[0] != '\0') {
            ctxAddToParent(ctx, ffonElementCreateString(textBuf));
        }
        return;
    }

    // Fallback: try to extract text from unknown block elements
    if (lxb_dom_node_first_child(node)) {
        // Check if it has block-level children
        bool hasBlockChildren = false;
        for (lxb_dom_node_t *child = lxb_dom_node_first_child(node);
             child != NULL;
             child = lxb_dom_node_next(child))
        {
            if (child->type == LXB_DOM_NODE_TYPE_ELEMENT) {
                lxb_dom_element_t *childEl = lxb_dom_interface_element(child);
                lxb_tag_id_t childTag = lxb_dom_element_tag_id(childEl);
                if (headingLevel(childTag) > 0 || childTag == LXB_TAG_P ||
                    childTag == LXB_TAG_UL || childTag == LXB_TAG_OL ||
                    childTag == LXB_TAG_TABLE || childTag == LXB_TAG_DL ||
                    isBlockContainer(childTag)) {
                    hasBlockChildren = true;
                    break;
                }
            }
        }

        if (hasBlockChildren) {
            processChildren(node, ctx);
        } else {
            char textBuf[MAX_TEXT_BUF];
            getNodeText(node, textBuf, sizeof(textBuf));
            if (textBuf[0] != '\0') {
                ctxAddToParent(ctx, ffonElementCreateString(textBuf));
            }
        }
    }
}

FfonElement** webbrowserHtmlToFfon(const char *html, const char *baseUrl, int *outCount) {
    *outCount = 0;
    if (!html || !html[0]) return NULL;

    lxb_html_document_t *doc = lxb_html_document_create();
    if (!doc) return NULL;

    lxb_status_t status = lxb_html_document_parse(doc,
        (const lxb_char_t *)html, strlen(html));
    if (status != LXB_STATUS_OK) {
        lxb_html_document_destroy(doc);
        return NULL;
    }

    lxb_dom_node_t *body = lxb_dom_interface_node(
        lxb_html_document_body_element(doc));
    if (!body) {
        lxb_html_document_destroy(doc);
        return NULL;
    }

    ParseContext ctx = {
        .result = NULL,
        .resultCount = 0,
        .resultCapacity = 0,
        .stackDepth = 0,
        .baseUrl = baseUrl,
    };

    processChildren(body, &ctx);

    lxb_html_document_destroy(doc);

    *outCount = ctx.resultCount;
    return ctx.result;
}
