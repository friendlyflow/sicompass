#pragma once

#include <vulkan/vulkan.h>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <hb.h>
#include <hb-ft.h>
#include <cglm/cglm.h>

#define FONT_ATLAS_SIZE 1024
#define MAX_TEXT_VERTICES 1024
#define MAX_FRAMES_IN_FLIGHT 2

// Forward declaration
typedef struct SiCompassApplication SiCompassApplication;

typedef struct GlyphInfo {
    vec2 size;
    vec2 bearing;
    uint32_t advance;
    vec2 uvMin;
    vec2 uvMax;
} GlyphInfo;

typedef struct TextVertex {
    vec3 pos;
    vec2 texCoord;
    vec3 color;
} TextVertex;

typedef struct FontRenderer {
    FT_Library ftLibrary;
    FT_Face ftFace;
    hb_font_t* hbFont;        // HarfBuzz font for text shaping
    hb_buffer_t* hbBuffer;    // HarfBuzz buffer for shaping operations

    VkImage fontAtlasImage;
    VkDeviceMemory fontAtlasMemory;
    VkImageView fontAtlasView;
    VkSampler fontAtlasSampler;

    GlyphInfo glyphs[256];  // Extended to support Latin-1 Supplement (includes é, à, ñ, etc.)

    float lineHeight;  // Font line height (ascender - descender)
    float ascender;    // Distance from baseline to top
    float descender;   // Distance from baseline to bottom (negative)
    float dpi;         // Screen DPI (96 * content scale)

    VkBuffer textVertexBuffer;
    VkDeviceMemory textVertexBufferMemory;

    VkDescriptorSetLayout textDescriptorSetLayout;
    VkDescriptorPool textDescriptorPool;
    VkDescriptorSet textDescriptorSets[MAX_FRAMES_IN_FLIGHT];

    VkPipelineLayout textPipelineLayout;
    VkPipeline textPipeline;

    uint32_t textVertexCount;
} FontRenderer;

// Initialization and cleanup
void initFreeType(SiCompassApplication* app);
void cleanupFontRenderer(SiCompassApplication* app);

// Font atlas creation
void createFontAtlas(SiCompassApplication* app);
void createFontAtlasView(SiCompassApplication* app);
void createFontAtlasSampler(SiCompassApplication* app);

// Buffer creation
void createTextVertexBuffer(SiCompassApplication* app);

// Descriptor sets
void createTextDescriptorSetLayout(SiCompassApplication* app);
void createTextDescriptorPool(SiCompassApplication* app);
void createTextDescriptorSets(SiCompassApplication* app);

// Pipeline creation
void createTextPipeline(SiCompassApplication* app);

// Text rendering
void beginTextRendering(SiCompassApplication* app);
void prepareTextForRendering(SiCompassApplication* app, const char* text,
                             float x, float y, float scale, vec3 color);

// Helper functions
void calculateTextBounds(SiCompassApplication* app, const char* text,
                        float x, float y, float scale,
                        float* outMinX, float* outMinY,
                        float* outMaxX, float* outMaxY);
float getTextScale(SiCompassApplication* app, float desiredSizePt);
float getWidthEM(SiCompassApplication* app, float scale);
float getLineHeight(SiCompassApplication* app, float scale, float padding);

// Drawing
void drawText(SiCompassApplication* app, VkCommandBuffer commandBuffer);
