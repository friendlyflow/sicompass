#include "checkmark.h"
#include "rectangle.h"
#include "main.h"
#include <math.h>

void prepareCheckmark(SiCompassApplication *app,
                      float x, float y, float size, uint32_t color) {
    RectangleRenderer *rr = app->rectangleRenderer;

    vec4 colorVec;
    colorVec[0] = ((color >> 24) & 0xFF) / 255.0f;
    colorVec[1] = ((color >> 16) & 0xFF) / 255.0f;
    colorVec[2] = ((color >> 8) & 0xFF) / 255.0f;
    colorVec[3] = (color & 0xFF) / 255.0f;

    // Scale from 24x24 Heroicons viewbox to actual size
    float s = size / 24.0f;

    // Heroicons check path: M4.5 12.75l6 6 9-13.5
    float ax = x + 4.5f * s,  ay = y + 12.75f * s;
    float bx = x + 10.5f * s, by = y + 18.75f * s;
    float cx = x + 19.5f * s, cy = y + 5.25f * s;

    // Stroke thickness (2.0 units in 24-unit space)
    float thickness = 2.0f * s;
    float half = thickness * 0.5f;

    // Perpendicular offset for segment A->B
    float dx1 = bx - ax, dy1 = by - ay;
    float len1 = sqrtf(dx1 * dx1 + dy1 * dy1);
    float nx1 = -dy1 / len1 * half;
    float ny1 =  dx1 / len1 * half;

    // Perpendicular offset for segment B->C
    float dx2 = cx - bx, dy2 = cy - by;
    float len2 = sqrtf(dx2 * dx2 + dy2 * dy2);
    float nx2 = -dy2 / len2 * half;
    float ny2 =  dx2 / len2 * half;

    // Need 12 vertices (2 quads = 4 triangles)
    if (rr->vertexCount + 12 > 6 * MAX_RECTANGLES) return;

    RectangleVertex *v = &rr->mappedVertexData[rr->vertexCount];

    // Set shared attributes: neutralize the SDF by using a huge rect
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

    // Quad 1: segment A->B (short downstroke)
    v[0].pos[0] = ax + nx1; v[0].pos[1] = ay + ny1;
    v[1].pos[0] = ax - nx1; v[1].pos[1] = ay - ny1;
    v[2].pos[0] = bx - nx1; v[2].pos[1] = by - ny1;
    v[3].pos[0] = ax + nx1; v[3].pos[1] = ay + ny1;
    v[4].pos[0] = bx - nx1; v[4].pos[1] = by - ny1;
    v[5].pos[0] = bx + nx1; v[5].pos[1] = by + ny1;

    // Quad 2: segment B->C (long upstroke)
    v[6].pos[0]  = bx + nx2; v[6].pos[1]  = by + ny2;
    v[7].pos[0]  = bx - nx2; v[7].pos[1]  = by - ny2;
    v[8].pos[0]  = cx - nx2; v[8].pos[1]  = cy - ny2;
    v[9].pos[0]  = bx + nx2; v[9].pos[1]  = by + ny2;
    v[10].pos[0] = cx - nx2; v[10].pos[1] = cy - ny2;
    v[11].pos[0] = cx + nx2; v[11].pos[1] = cy + ny2;

    rr->vertexCount += 12;
}
