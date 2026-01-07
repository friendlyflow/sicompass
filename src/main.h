#pragma once

#include <vulkan/vulkan.h>
#include <SDL3/SDL.h>
#include <SDL3/SDL_vulkan.h>

#include <ft2build.h>
#include FT_FREETYPE_H

#include <cglm/cglm.h>
#include <cglm/mat4.h>
#include <cglm/cam.h>
#include <cglm/affine.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <time.h>

#include "view.h"

#define MAX_FRAMES_IN_FLIGHT 2

#define FONT_ATLAS_SIZE 512
#define MAX_TEXT_VERTICES 1024

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

typedef struct BackgroundVertex {
    vec2 pos;
    vec4 color;
    vec2 cornerRadius;  // x = radius, y = unused (for alignment)
    vec2 rectSize;      // width and height of the rectangle
    vec2 rectOrigin;    // top-left corner (minX, minY) of the rectangle
} BackgroundVertex;

typedef struct FontRenderer {
    FT_Library ftLibrary;
    FT_Face ftFace;

    VkImage fontAtlasImage;
    VkDeviceMemory fontAtlasMemory;
    VkImageView fontAtlasView;
    VkSampler fontAtlasSampler;

    GlyphInfo glyphs[128];

    float lineHeight;  // Font line height (ascender - descender)
    float ascender;    // Distance from baseline to top
    float descender;   // Distance from baseline to bottom (negative)

    VkBuffer textVertexBuffer;
    VkDeviceMemory textVertexBufferMemory;

    VkDescriptorSetLayout textDescriptorSetLayout;
    VkDescriptorPool textDescriptorPool;
    VkDescriptorSet textDescriptorSets[MAX_FRAMES_IN_FLIGHT];

    VkPipelineLayout textPipelineLayout;
    VkPipeline textPipeline;

    uint32_t textVertexCount;

    VkBuffer backgroundVertexBuffer;
    VkDeviceMemory backgroundVertexBufferMemory;

    VkPipelineLayout backgroundPipelineLayout;
    VkPipeline backgroundPipeline;

    uint32_t backgroundVertexCount;
} FontRenderer;

typedef struct {
    vec3 pos;
    vec3 color;
    vec2 texCoord;
} Vertex;

// if it doesn't work:
// typedef struct {
//     vec3 pos;
//     vec3 color;
//     vec2 texCoord;
// } Vertex;

typedef struct SiCompassApplication {
    SDL_Window* window;

    VkInstance instance;
    VkDebugUtilsMessengerEXT debugMessenger;
    VkSurfaceKHR surface;

    VkPhysicalDevice physicalDevice;
    // VkSampleCountFlagBits msaaSamples;
    VkDevice device;

    VkQueue graphicsQueue;
    VkQueue presentQueue;

    VkSwapchainKHR swapChain;
    VkImage* swapChainImages;
    uint32_t swapChainImageCount;
    VkFormat swapChainImageFormat;
    VkExtent2D swapChainExtent;
    VkImageView* swapChainImageViews;
    uint32_t swapChainImageViewCount;
    VkFramebuffer* swapChainFramebuffers;
    uint32_t swapChainFramebufferCount;

    VkRenderPass renderPass;
    VkDescriptorSetLayout descriptorSetLayout;
    VkPipelineLayout pipelineLayout;
    VkPipeline graphicsPipeline;

    VkCommandPool commandPool;

    // VkImage colorImage;
    // VkDeviceMemory colorImageMemory;
    // VkImageView colorImageView;

    VkImage depthImage;
    VkDeviceMemory depthImageMemory;
    VkImageView depthImageView;

    // uint32_t mipLevels;
    VkImage textureImage;
    VkDeviceMemory textureImageMemory;
    VkImageView textureImageView;
    VkSampler textureSampler;

    // Vertex* vertices;
    // size_t vertexCount;
    // size_t vertexCapacity;
    // uint32_t* indices;
    // size_t indexCount;
    // size_t indexCapacity;
    VkBuffer vertexBuffer;
    VkDeviceMemory vertexBufferMemory;
    VkBuffer indexBuffer;
    VkDeviceMemory indexBufferMemory;

    VkBuffer uniformBuffers[MAX_FRAMES_IN_FLIGHT];
    VkDeviceMemory uniformBuffersMemory[MAX_FRAMES_IN_FLIGHT];
    void* uniformBuffersMapped[MAX_FRAMES_IN_FLIGHT];
    uint32_t uniformBufferCount;

    VkDescriptorPool descriptorPool;
    VkDescriptorSet descriptorSets[MAX_FRAMES_IN_FLIGHT];
    uint32_t descriptorSetCount;

    VkCommandBuffer commandBuffers[MAX_FRAMES_IN_FLIGHT];
    uint32_t commandBufferCount;

    VkSemaphore imageAvailableSemaphores[MAX_FRAMES_IN_FLIGHT];
    VkSemaphore renderFinishedSemaphores[MAX_FRAMES_IN_FLIGHT];
    VkFence inFlightFences[MAX_FRAMES_IN_FLIGHT];
    // uint32_t syncObjectCount;
    uint32_t currentFrame;

    bool framebufferResized;
    bool running;
    clock_t startTime;

    FontRenderer* fontRenderer;
} SiCompassApplication;

void beginTextRendering(SiCompassApplication* app);
void prepareBackgroundForText(SiCompassApplication* app, const char* text, float x, float y, float scale, vec4 bgColor, float cornerRadius, float padding);
void prepareTextForRendering(SiCompassApplication* app, const char* text, float x, float y, float scale, vec3 color);
void drawFrame(SiCompassApplication* app);