#pragma once

#include <vulkan/vulkan.h>
#include <cglm/cglm.h>
#include <stdbool.h>

#define MAX_FRAMES_IN_FLIGHT 2

// Forward declaration
typedef struct SiCompassApplication SiCompassApplication;

typedef struct ImageVertex {
    vec2 pos;
    vec2 texCoord;
} ImageVertex;

#define MAX_CACHED_IMAGES 16
#define MAX_VISIBLE_IMAGES 16

typedef struct CachedTexture {
    VkImage image;
    VkDeviceMemory memory;
    VkImageView imageView;
    VkSampler sampler;
    VkDescriptorSet descriptorSets[MAX_FRAMES_IN_FLIGHT];
    char path[4096];
    int width;
    int height;
    bool loaded;
    uint32_t lastUsedFrame;
} CachedTexture;

typedef struct ImageDrawCall {
    int cacheIndex;
    ImageVertex vertices[6];
} ImageDrawCall;

typedef struct ImageRenderer {
    CachedTexture cache[MAX_CACHED_IMAGES];
    uint32_t frameCounter;

    ImageDrawCall drawCalls[MAX_VISIBLE_IMAGES];
    uint32_t drawCallCount;

    int textureWidth;
    int textureHeight;
    int lastLoadedCacheIndex;

    VkBuffer vertexBuffer;
    VkDeviceMemory vertexBufferMemory;

    VkDescriptorSetLayout descriptorSetLayout;
    VkDescriptorPool descriptorPool;

    VkPipelineLayout pipelineLayout;
    VkPipeline pipeline;
} ImageRenderer;

// Initialization and cleanup
void initImageRenderer(SiCompassApplication* app);
void cleanupImageRenderer(SiCompassApplication* app);

// Texture loading
bool loadImageTexture(SiCompassApplication* app, const char* path);

// Rendering
void beginImageRendering(SiCompassApplication* app);
void prepareImage(SiCompassApplication* app, float x, float y, float width, float height);
void drawImageQuad(SiCompassApplication* app, VkCommandBuffer commandBuffer);

// Render pass and framebuffers (general Vulkan setup, not image-specific)
void createRenderPass(SiCompassApplication* app);
void createFramebuffers(SiCompassApplication* app);

// Command pool and buffers
void createCommandPool(SiCompassApplication* app);
void createCommandBuffers(SiCompassApplication* app);

// Depth resources
void createDepthResources(SiCompassApplication* app);
VkFormat findSupportedFormat(SiCompassApplication* app, const VkFormat* candidates, size_t candidateCount,
                             VkImageTiling tiling, VkFormatFeatureFlags features);
VkFormat findDepthFormat(SiCompassApplication* app);

// Synchronization
void createSyncObjects(SiCompassApplication* app);
