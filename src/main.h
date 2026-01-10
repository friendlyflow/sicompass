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

#include "text.h"
#include "image.h"

#define MAX_FRAMES_IN_FLIGHT 2

// Forward declarations
typedef struct AppRenderer AppRenderer;


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

    AppRenderer* appRenderer;
} SiCompassApplication;

// Core application functions
void drawFrame(SiCompassApplication* app);
