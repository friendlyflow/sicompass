/*
 * Tests for text.c math/metric functions:
 * - getTextScale
 * - getWidthEM
 * - getLineHeight
 *
 * These functions only read FontRenderer metric fields,
 * no Vulkan initialization needed.
 */

#include <unity.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

/* ============================================
 * Minimal type stubs (only metric fields used)
 * ============================================ */

typedef struct {
    float size[2];
    float bearing[2];
    uint32_t advance;
    float uvMin[2];
    float uvMax[2];
} GlyphInfo;

typedef struct {
    GlyphInfo glyphs[256];
    float lineHeight;
    float ascender;
    float descender;
    float dpi;
} FontRenderer;

typedef struct {
    FontRenderer *fontRenderer;
} SiCompassApplication;

/* ============================================
 * Functions under test (from text.c)
 * ============================================ */

float getTextScale(SiCompassApplication* app, float desiredSizePt) {
    FontRenderer* fr = app->fontRenderer;
    float desiredHeightPx = desiredSizePt * fr->dpi / 72.0f;
    return desiredHeightPx / fr->lineHeight;
}

float getWidthEM(SiCompassApplication* app, float scale) {
    FontRenderer* fr = app->fontRenderer;
    GlyphInfo* g = &fr->glyphs[(int)'M'];
    return g->advance * scale;
}

float getLineHeight(SiCompassApplication* app, float scale, float padding) {
    FontRenderer* fr = app->fontRenderer;
    return fr->lineHeight * scale + (padding * 2.0f);
}

/* ============================================
 * Test helpers
 * ============================================ */

static FontRenderer g_fr;
static SiCompassApplication g_app;

static void setupFont(float dpi, float lineHeight, float ascender, float descender) {
    memset(&g_fr, 0, sizeof(g_fr));
    g_fr.dpi = dpi;
    g_fr.lineHeight = lineHeight;
    g_fr.ascender = ascender;
    g_fr.descender = descender;
    g_app.fontRenderer = &g_fr;
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    // Default: 96 DPI, lineHeight=20, typical metrics
    setupFont(96.0f, 20.0f, 15.0f, -5.0f);
}

void tearDown(void) {}

/* ============================================
 * getTextScale tests
 * ============================================ */

void test_getTextScale_12pt_96dpi(void) {
    // 12pt at 96 DPI: pixels = 12 * 96 / 72 = 16
    // scale = 16 / 20 = 0.8
    float scale = getTextScale(&g_app, 12.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 0.8f, scale);
}

void test_getTextScale_24pt_96dpi(void) {
    // 24pt at 96 DPI: pixels = 24 * 96 / 72 = 32
    // scale = 32 / 20 = 1.6
    float scale = getTextScale(&g_app, 24.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 1.6f, scale);
}

void test_getTextScale_12pt_144dpi(void) {
    setupFont(144.0f, 20.0f, 15.0f, -5.0f);
    // 12pt at 144 DPI: pixels = 12 * 144 / 72 = 24
    // scale = 24 / 20 = 1.2
    float scale = getTextScale(&g_app, 12.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 1.2f, scale);
}

void test_getTextScale_proportional_to_size(void) {
    float scale12 = getTextScale(&g_app, 12.0f);
    float scale24 = getTextScale(&g_app, 24.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, scale12 * 2.0f, scale24);
}

void test_getTextScale_proportional_to_dpi(void) {
    setupFont(96.0f, 20.0f, 15.0f, -5.0f);
    float scale96 = getTextScale(&g_app, 12.0f);

    setupFont(192.0f, 20.0f, 15.0f, -5.0f);
    float scale192 = getTextScale(&g_app, 12.0f);

    TEST_ASSERT_FLOAT_WITHIN(0.001f, scale96 * 2.0f, scale192);
}

void test_getTextScale_inversely_proportional_to_lineHeight(void) {
    setupFont(96.0f, 20.0f, 15.0f, -5.0f);
    float scale20 = getTextScale(&g_app, 12.0f);

    setupFont(96.0f, 40.0f, 30.0f, -10.0f);
    float scale40 = getTextScale(&g_app, 12.0f);

    TEST_ASSERT_FLOAT_WITHIN(0.001f, scale20 / 2.0f, scale40);
}

/* ============================================
 * getWidthEM tests
 * ============================================ */

void test_getWidthEM_basic(void) {
    g_fr.glyphs[(int)'M'].advance = 10;
    float width = getWidthEM(&g_app, 1.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 10.0f, width);
}

void test_getWidthEM_scaled(void) {
    g_fr.glyphs[(int)'M'].advance = 10;
    float width = getWidthEM(&g_app, 2.5f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 25.0f, width);
}

void test_getWidthEM_zero_scale(void) {
    g_fr.glyphs[(int)'M'].advance = 10;
    float width = getWidthEM(&g_app, 0.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 0.0f, width);
}

/* ============================================
 * getLineHeight tests
 * ============================================ */

void test_getLineHeight_no_padding(void) {
    float h = getLineHeight(&g_app, 1.0f, 0.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 20.0f, h);
}

void test_getLineHeight_with_padding(void) {
    // lineHeight * scale + padding * 2 = 20 * 1.0 + 4 * 2 = 28
    float h = getLineHeight(&g_app, 1.0f, 4.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 28.0f, h);
}

void test_getLineHeight_scaled(void) {
    // 20 * 2.0 + 0 = 40
    float h = getLineHeight(&g_app, 2.0f, 0.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 40.0f, h);
}

void test_getLineHeight_scaled_with_padding(void) {
    // 20 * 0.5 + 3 * 2 = 10 + 6 = 16
    float h = getLineHeight(&g_app, 0.5f, 3.0f);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 16.0f, h);
}

void test_getLineHeight_different_font(void) {
    setupFont(96.0f, 30.0f, 22.0f, -8.0f);
    float h = getLineHeight(&g_app, 1.0f, 2.0f);
    // 30 * 1.0 + 2 * 2 = 34
    TEST_ASSERT_FLOAT_WITHIN(0.001f, 34.0f, h);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // getTextScale
    RUN_TEST(test_getTextScale_12pt_96dpi);
    RUN_TEST(test_getTextScale_24pt_96dpi);
    RUN_TEST(test_getTextScale_12pt_144dpi);
    RUN_TEST(test_getTextScale_proportional_to_size);
    RUN_TEST(test_getTextScale_proportional_to_dpi);
    RUN_TEST(test_getTextScale_inversely_proportional_to_lineHeight);

    // getWidthEM
    RUN_TEST(test_getWidthEM_basic);
    RUN_TEST(test_getWidthEM_scaled);
    RUN_TEST(test_getWidthEM_zero_scale);

    // getLineHeight
    RUN_TEST(test_getLineHeight_no_padding);
    RUN_TEST(test_getLineHeight_with_padding);
    RUN_TEST(test_getLineHeight_scaled);
    RUN_TEST(test_getLineHeight_scaled_with_padding);
    RUN_TEST(test_getLineHeight_different_font);

    return UNITY_END();
}
