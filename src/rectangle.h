#pragma once

#include <vulkan/vulkan.h>
#include <cglm/cglm.h>

#define MAX_FRAMES_IN_FLIGHT 2

// Forward declaration
typedef struct SiCompassApplication SiCompassApplication;

typedef struct RectangleVertex {
    vec2 pos;
    vec4 color;
    vec2 cornerRadius;  // x = radius, y = unused (for alignment)
    vec2 rectSize;      // width and height of the rectangle
    vec2 rectOrigin;    // top-left corner (minX, minY) of the rectangle
} RectangleVertex;

typedef struct RectangleRenderer {
    VkBuffer vertexBuffer;
    VkDeviceMemory vertexBufferMemory;
    VkPipelineLayout pipelineLayout;
    VkPipeline pipeline;
    uint32_t vertexCount;
} RectangleRenderer;

// Initialization and cleanup
void createRectangleVertexBuffer(SiCompassApplication* app);
void createRectanglePipeline(SiCompassApplication* app);
void cleanupRectangleRenderer(SiCompassApplication* app);

// Rectangle rendering
void prepareRectangle(SiCompassApplication* app,
                     float x, float y, float width, float height,
                     vec4 color, float cornerRadius);

// Drawing
void drawRectangle(SiCompassApplication* app, VkCommandBuffer commandBuffer);
