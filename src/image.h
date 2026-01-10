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

// Drawing
void drawImage(SiCompassApplication* app, VkCommandBuffer commandBuffer);
