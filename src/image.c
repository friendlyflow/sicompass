#include "image.h"
#include "main.h"
#include <stb_image.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Forward declarations of helper functions from main.c
extern void createBuffer(SiCompassApplication* app, VkDeviceSize size, VkBufferUsageFlags usage,
                        VkMemoryPropertyFlags properties, VkBuffer* buffer, VkDeviceMemory* bufferMemory);
extern void createImage(SiCompassApplication* app, uint32_t width, uint32_t height, VkFormat format,
                       VkImageTiling tiling, VkImageUsageFlags usage, VkMemoryPropertyFlags properties,
                       VkImage* image, VkDeviceMemory* imageMemory);
extern void transitionImageLayout(SiCompassApplication* app, VkImage image, VkFormat format,
                                 VkImageLayout oldLayout, VkImageLayout newLayout);
extern void copyBufferToImage(SiCompassApplication* app, VkBuffer buffer, VkImage image,
                             uint32_t width, uint32_t height);
extern VkImageView createImageView(SiCompassApplication* app, VkImage image, VkFormat format, VkImageAspectFlags aspectFlags);

void createTextureImage(SiCompassApplication* app) {
    int texWidth, texHeight, texChannels;
    stbi_uc* pixels = stbi_load("textures/texture.jpg", &texWidth, &texHeight, &texChannels, STBI_rgb_alpha);
    VkDeviceSize imageSize = texWidth * texHeight * 4;

    if (!pixels) {
        fprintf(stderr, "Failed to load texture image!\n");
        exit(EXIT_FAILURE);
    }

    VkBuffer stagingBuffer;
    VkDeviceMemory stagingBufferMemory;
    createBuffer(app, imageSize, VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                 &stagingBuffer, &stagingBufferMemory);

    void* data;
    vkMapMemory(app->device, stagingBufferMemory, 0, imageSize, 0, &data);
    memcpy(data, pixels, (size_t)imageSize);
    vkUnmapMemory(app->device, stagingBufferMemory);

    stbi_image_free(pixels);

    createImage(app, texWidth, texHeight, VK_FORMAT_R8G8B8A8_SRGB, VK_IMAGE_TILING_OPTIMAL,
                VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT,
                &app->textureImage, &app->textureImageMemory);

    transitionImageLayout(app, app->textureImage, VK_FORMAT_R8G8B8A8_SRGB,
                         VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    copyBufferToImage(app, stagingBuffer, app->textureImage, (uint32_t)texWidth, (uint32_t)texHeight);
    transitionImageLayout(app, app->textureImage, VK_FORMAT_R8G8B8A8_SRGB,
                         VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);

    vkDestroyBuffer(app->device, stagingBuffer, NULL);
    vkFreeMemory(app->device, stagingBufferMemory, NULL);
}

void createTextureImageView(SiCompassApplication* app) {
    app->textureImageView = createImageView(app, app->textureImage, VK_FORMAT_R8G8B8A8_SRGB, VK_IMAGE_ASPECT_COLOR_BIT);
}

void createTextureSampler(SiCompassApplication* app) {
    VkPhysicalDeviceProperties properties = {0};
    vkGetPhysicalDeviceProperties(app->physicalDevice, &properties);

    VkSamplerCreateInfo samplerInfo = {0};
    samplerInfo.sType = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO;
    samplerInfo.magFilter = VK_FILTER_LINEAR;
    samplerInfo.minFilter = VK_FILTER_LINEAR;
    samplerInfo.addressModeU = VK_SAMPLER_ADDRESS_MODE_REPEAT;
    samplerInfo.addressModeV = VK_SAMPLER_ADDRESS_MODE_REPEAT;
    samplerInfo.addressModeW = VK_SAMPLER_ADDRESS_MODE_REPEAT;
    samplerInfo.anisotropyEnable = VK_TRUE;
    samplerInfo.maxAnisotropy = properties.limits.maxSamplerAnisotropy;
    samplerInfo.borderColor = VK_BORDER_COLOR_INT_OPAQUE_BLACK;
    samplerInfo.unnormalizedCoordinates = VK_FALSE;
    samplerInfo.compareEnable = VK_FALSE;
    samplerInfo.compareOp = VK_COMPARE_OP_ALWAYS;
    samplerInfo.mipmapMode = VK_SAMPLER_MIPMAP_MODE_LINEAR;

    if (vkCreateSampler(app->device, &samplerInfo, NULL, &app->textureSampler) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create texture sampler!\n");
        exit(EXIT_FAILURE);
    }
}

void cleanupTextureResources(SiCompassApplication* app) {
    vkDestroySampler(app->device, app->textureSampler, NULL);
    vkDestroyImageView(app->device, app->textureImageView, NULL);
    vkDestroyImage(app->device, app->textureImage, NULL);
    vkFreeMemory(app->device, app->textureImageMemory, NULL);
}

void drawImage(SiCompassApplication* app, VkCommandBuffer commandBuffer) {
    // Draw image
    vkCmdBindPipeline(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS, app->graphicsPipeline);

    VkViewport viewport = {0};
    viewport.x = 0.0f;
    viewport.y = 0.0f;
    viewport.width = (float)app->swapChainExtent.width;
    viewport.height = (float)app->swapChainExtent.height;
    viewport.minDepth = 0.0f;
    viewport.maxDepth = 1.0f;
    vkCmdSetViewport(commandBuffer, 0, 1, &viewport);

    VkRect2D scissor = {0};
    scissor.offset = (VkOffset2D){0, 0};
    scissor.extent = app->swapChainExtent;
    vkCmdSetScissor(commandBuffer, 0, 1, &scissor);

    VkBuffer vertexBuffers[] = {app->vertexBuffer};
    VkDeviceSize offsets[] = {0};
    vkCmdBindVertexBuffers(commandBuffer, 0, 1, vertexBuffers, offsets);

    vkCmdBindIndexBuffer(commandBuffer, app->indexBuffer, 0, VK_INDEX_TYPE_UINT16);

    extern const uint32_t indexCount;
    vkCmdBindDescriptorSets(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS, app->pipelineLayout,
                           0, 1, &app->descriptorSets[app->currentFrame], 0, NULL);

    vkCmdDrawIndexed(commandBuffer, (uint32_t)indexCount, 1, 0, 0, 0);
}
