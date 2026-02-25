/*
 * Tests for checkmark.c:
 * - prepareCheckmark (geometry calculation)
 *
 * Uses a fake RectangleRenderer with stack-allocated vertex buffer
 * to verify vertex positions and colors.
 */

#include <unity.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

/* ============================================
 * Minimal type stubs
 * ============================================ */

#define MAX_RECTANGLES 200

typedef struct {
    float pos[2];
    float color[4];
    float cornerRadius[2];
    float rectSize[2];
    float rectOrigin[2];
} RectangleVertex;

typedef struct {
    uint32_t vertexCount;
    RectangleVertex *mappedVertexData;
} RectangleRenderer;

typedef struct {
    RectangleRenderer *rectangleRenderer;
} SiCompassApplication;

/* ============================================
 * Function under test (from checkmark.c)
 * ============================================ */

void prepareCheckmark(SiCompassApplication *app,
                      float x, float y, float size, uint32_t color) {
    RectangleRenderer *rr = app->rectangleRenderer;

    float colorVec[4];
    colorVec[0] = ((color >> 24) & 0xFF) / 255.0f;
    colorVec[1] = ((color >> 16) & 0xFF) / 255.0f;
    colorVec[2] = ((color >> 8) & 0xFF) / 255.0f;
    colorVec[3] = (color & 0xFF) / 255.0f;

    float s = size / 24.0f;

    float ax = x + 4.5f * s,  ay = y + 12.75f * s;
    float bx = x + 10.5f * s, by = y + 18.75f * s;
    float cx = x + 19.5f * s, cy = y + 5.25f * s;

    float thickness = 2.0f * s;
    float half = thickness * 0.5f;

    float dx1 = bx - ax, dy1 = by - ay;
    float len1 = sqrtf(dx1 * dx1 + dy1 * dy1);
    float nx1 = -dy1 / len1 * half;
    float ny1 =  dx1 / len1 * half;

    float dx2 = cx - bx, dy2 = cy - by;
    float len2 = sqrtf(dx2 * dx2 + dy2 * dy2);
    float nx2 = -dy2 / len2 * half;
    float ny2 =  dx2 / len2 * half;

    if (rr->vertexCount + 12 > 6 * MAX_RECTANGLES) return;

    RectangleVertex *v = &rr->mappedVertexData[rr->vertexCount];

    for (int i = 0; i < 12; i++) {
        v[i].color[0] = colorVec[0];
        v[i].color[1] = colorVec[1];
        v[i].color[2] = colorVec[2];
        v[i].color[3] = colorVec[3];
        v[i].cornerRadius[0] = 0.0f;
        v[i].cornerRadius[1] = 0.0f;
        v[i].rectSize[0] = 10000.0f;
        v[i].rectSize[1] = 10000.0f;
        v[i].rectOrigin[0] = 0.0f;
        v[i].rectOrigin[1] = 0.0f;
    }

    v[0].pos[0] = ax + nx1; v[0].pos[1] = ay + ny1;
    v[1].pos[0] = ax - nx1; v[1].pos[1] = ay - ny1;
    v[2].pos[0] = bx - nx1; v[2].pos[1] = by - ny1;
    v[3].pos[0] = ax + nx1; v[3].pos[1] = ay + ny1;
    v[4].pos[0] = bx - nx1; v[4].pos[1] = by - ny1;
    v[5].pos[0] = bx + nx1; v[5].pos[1] = by + ny1;

    v[6].pos[0]  = bx + nx2; v[6].pos[1]  = by + ny2;
    v[7].pos[0]  = bx - nx2; v[7].pos[1]  = by - ny2;
    v[8].pos[0]  = cx - nx2; v[8].pos[1]  = cy - ny2;
    v[9].pos[0]  = bx + nx2; v[9].pos[1]  = by + ny2;
    v[10].pos[0] = cx - nx2; v[10].pos[1] = cy - ny2;
    v[11].pos[0] = cx + nx2; v[11].pos[1] = cy + ny2;

    rr->vertexCount += 12;
}

/* ============================================
 * Test helpers
 * ============================================ */

static RectangleVertex g_vertices[6 * MAX_RECTANGLES];
static RectangleRenderer g_rr;
static SiCompassApplication g_app;

static void setupCheckmark(void) {
    memset(g_vertices, 0, sizeof(g_vertices));
    g_rr.vertexCount = 0;
    g_rr.mappedVertexData = g_vertices;
    g_app.rectangleRenderer = &g_rr;
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    setupCheckmark();
}

void tearDown(void) {}

/* ============================================
 * prepareCheckmark tests
 * ============================================ */

void test_prepareCheckmark_adds_12_vertices(void) {
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);
    TEST_ASSERT_EQUAL_UINT32(12, g_rr.vertexCount);
}

void test_prepareCheckmark_color_white(void) {
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);
    for (int i = 0; i < 12; i++) {
        TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, g_vertices[i].color[0]);
        TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, g_vertices[i].color[1]);
        TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, g_vertices[i].color[2]);
        TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, g_vertices[i].color[3]);
    }
}

void test_prepareCheckmark_color_red(void) {
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFF0000FF);
    TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, g_vertices[0].color[0]);  // R
    TEST_ASSERT_FLOAT_WITHIN(0.01f, 0.0f, g_vertices[0].color[1]);  // G
    TEST_ASSERT_FLOAT_WITHIN(0.01f, 0.0f, g_vertices[0].color[2]);  // B
    TEST_ASSERT_FLOAT_WITHIN(0.01f, 1.0f, g_vertices[0].color[3]);  // A
}

void test_prepareCheckmark_corner_radius_zero(void) {
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);
    for (int i = 0; i < 12; i++) {
        TEST_ASSERT_FLOAT_WITHIN(0.001f, 0.0f, g_vertices[i].cornerRadius[0]);
    }
}

void test_prepareCheckmark_vertices_in_bounds(void) {
    float x = 10.0f, y = 20.0f, size = 48.0f;
    prepareCheckmark(&g_app, x, y, size, 0xFFFFFFFF);

    for (int i = 0; i < 12; i++) {
        // All vertices should be roughly within the bounding box
        // Allow some margin for stroke thickness
        TEST_ASSERT_TRUE(g_vertices[i].pos[0] >= x - size * 0.2f);
        TEST_ASSERT_TRUE(g_vertices[i].pos[0] <= x + size * 1.2f);
        TEST_ASSERT_TRUE(g_vertices[i].pos[1] >= y - size * 0.2f);
        TEST_ASSERT_TRUE(g_vertices[i].pos[1] <= y + size * 1.2f);
    }
}

void test_prepareCheckmark_size_scaling(void) {
    // Small checkmark
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);
    float small_max_x = 0;
    for (int i = 0; i < 12; i++) {
        if (g_vertices[i].pos[0] > small_max_x) small_max_x = g_vertices[i].pos[0];
    }

    // Large checkmark (2x)
    setupCheckmark();
    prepareCheckmark(&g_app, 0, 0, 48.0f, 0xFFFFFFFF);
    float large_max_x = 0;
    for (int i = 0; i < 12; i++) {
        if (g_vertices[i].pos[0] > large_max_x) large_max_x = g_vertices[i].pos[0];
    }

    // Large should be ~2x the small
    TEST_ASSERT_FLOAT_WITHIN(1.0f, small_max_x * 2.0f, large_max_x);
}

void test_prepareCheckmark_offset_position(void) {
    prepareCheckmark(&g_app, 100.0f, 200.0f, 24.0f, 0xFFFFFFFF);

    // All vertices should be offset by (100, 200)
    for (int i = 0; i < 12; i++) {
        TEST_ASSERT_TRUE(g_vertices[i].pos[0] >= 100.0f);
        TEST_ASSERT_TRUE(g_vertices[i].pos[1] >= 200.0f);
    }
}

void test_prepareCheckmark_two_quads(void) {
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);

    // First quad (vertices 0-5): short downstroke A->B
    // Vertices 0,3 should be same (shared corner)
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[0].pos[0], g_vertices[3].pos[0]);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[0].pos[1], g_vertices[3].pos[1]);

    // Vertices 2,4 should be same (shared corner)
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[2].pos[0], g_vertices[4].pos[0]);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[2].pos[1], g_vertices[4].pos[1]);

    // Second quad (vertices 6-11): long upstroke B->C
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[6].pos[0], g_vertices[9].pos[0]);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[6].pos[1], g_vertices[9].pos[1]);

    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[8].pos[0], g_vertices[10].pos[0]);
    TEST_ASSERT_FLOAT_WITHIN(0.001f, g_vertices[8].pos[1], g_vertices[10].pos[1]);
}

void test_prepareCheckmark_buffer_full_noop(void) {
    // Fill buffer to near capacity
    g_rr.vertexCount = 6 * MAX_RECTANGLES - 11; // only room for 11, needs 12
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);
    // Should not have added vertices
    TEST_ASSERT_EQUAL_UINT32(6 * MAX_RECTANGLES - 11, g_rr.vertexCount);
}

void test_prepareCheckmark_multiple_calls(void) {
    prepareCheckmark(&g_app, 0, 0, 24.0f, 0xFFFFFFFF);
    prepareCheckmark(&g_app, 50, 50, 24.0f, 0xFF0000FF);
    TEST_ASSERT_EQUAL_UINT32(24, g_rr.vertexCount);

    // Second checkmark vertices should be offset
    TEST_ASSERT_TRUE(g_vertices[12].pos[0] >= 50.0f);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_prepareCheckmark_adds_12_vertices);
    RUN_TEST(test_prepareCheckmark_color_white);
    RUN_TEST(test_prepareCheckmark_color_red);
    RUN_TEST(test_prepareCheckmark_corner_radius_zero);
    RUN_TEST(test_prepareCheckmark_vertices_in_bounds);
    RUN_TEST(test_prepareCheckmark_size_scaling);
    RUN_TEST(test_prepareCheckmark_offset_position);
    RUN_TEST(test_prepareCheckmark_two_quads);
    RUN_TEST(test_prepareCheckmark_buffer_full_noop);
    RUN_TEST(test_prepareCheckmark_multiple_calls);

    return UNITY_END();
}
