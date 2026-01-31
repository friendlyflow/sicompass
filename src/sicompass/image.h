#pragma once

#include <vulkan/vulkan.h>

// Forward declaration
typedef struct SiCompassApplication SiCompassApplication;

// Vertex description functions
VkVertexInputBindingDescription getBindingDescription(void);
void getAttributeDescriptions(VkVertexInputAttributeDescription* attributeDescriptions);

// Texture image creation and management
void createTextureImage(SiCompassApplication* app);
void createTextureImageView(SiCompassApplication* app);
void createTextureSampler(SiCompassApplication* app);
void cleanupTextureResources(SiCompassApplication* app);

// Uniform buffers
void createUniformBuffers(SiCompassApplication* app);
void updateUniformBuffer(SiCompassApplication* app, uint32_t currentImage);

// Image vertex and index buffers
void createImageVertexBuffer(SiCompassApplication* app);
void createImageIndexBuffer(SiCompassApplication* app);

// Image descriptor layout, pool and sets
void createImageDescriptorSetLayout(SiCompassApplication* app);
void createImageDescriptorPool(SiCompassApplication* app);
void createImageDescriptorSets(SiCompassApplication* app);

// Image pipeline
void createImagePipeline(SiCompassApplication* app);

// Render pass and framebuffers
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

// Drawing
void drawImage(SiCompassApplication* app, VkCommandBuffer commandBuffer);
