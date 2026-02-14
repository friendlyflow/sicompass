#include "image.h"
#include "main.h"
#include <stb_image.h>

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
extern void copyBuffer(SiCompassApplication* app, VkBuffer srcBuffer, VkBuffer dstBuffer, VkDeviceSize size);
extern VkImageView createImageView(SiCompassApplication* app, VkImage image, VkFormat format, VkImageAspectFlags aspectFlags);
extern char* readFile(const char* filename, size_t* fileSize);
extern VkShaderModule createShaderModule(VkDevice device, const char* code, size_t codeSize);
extern QueueFamilyIndices findQueueFamilies(VkPhysicalDevice device, VkSurfaceKHR surface);

// ============================================================================
// IMAGE RENDERER FUNCTIONS
// ============================================================================

static void createImageVertexBuffer(SiCompassApplication* app) {
    ImageRenderer* ir = app->imageRenderer;
    VkDeviceSize bufferSize = sizeof(ImageVertex) * 6 * MAX_VISIBLE_IMAGES;

    createBuffer(app, bufferSize, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                 &ir->vertexBuffer, &ir->vertexBufferMemory);
}

static void createImageDescriptorSetLayout(SiCompassApplication* app) {
    ImageRenderer* ir = app->imageRenderer;

    VkDescriptorSetLayoutBinding samplerBinding = {0};
    samplerBinding.binding = 0;
    samplerBinding.descriptorCount = 1;
    samplerBinding.descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
    samplerBinding.stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT;

    VkDescriptorSetLayoutCreateInfo layoutInfo = {0};
    layoutInfo.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO;
    layoutInfo.bindingCount = 1;
    layoutInfo.pBindings = &samplerBinding;

    if (vkCreateDescriptorSetLayout(app->device, &layoutInfo, NULL,
                                    &ir->descriptorSetLayout) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create image descriptor set layout!\n");
        exit(EXIT_FAILURE);
    }
}

static void createImageDescriptorPool(SiCompassApplication* app) {
    ImageRenderer* ir = app->imageRenderer;
    uint32_t maxSets = MAX_CACHED_IMAGES * MAX_FRAMES_IN_FLIGHT;

    VkDescriptorPoolSize poolSize = {0};
    poolSize.type = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
    poolSize.descriptorCount = maxSets;

    VkDescriptorPoolCreateInfo poolInfo = {0};
    poolInfo.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO;
    poolInfo.poolSizeCount = 1;
    poolInfo.pPoolSizes = &poolSize;
    poolInfo.maxSets = maxSets;
    poolInfo.flags = VK_DESCRIPTOR_POOL_CREATE_FREE_DESCRIPTOR_SET_BIT;

    if (vkCreateDescriptorPool(app->device, &poolInfo, NULL,
                               &ir->descriptorPool) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create image descriptor pool!\n");
        exit(EXIT_FAILURE);
    }
}

static void createImagePipeline(SiCompassApplication* app) {
    ImageRenderer* ir = app->imageRenderer;

    size_t vertSize, fragSize;
    char* vertCode = readFile("shaders/image_vert.spv", &vertSize);
    char* fragCode = readFile("shaders/image_frag.spv", &fragSize);

    VkShaderModule vertModule = createShaderModule(app->device, vertCode, vertSize);
    VkShaderModule fragModule = createShaderModule(app->device, fragCode, fragSize);

    VkPipelineShaderStageCreateInfo stages[2] = {0};
    stages[0].sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
    stages[0].stage = VK_SHADER_STAGE_VERTEX_BIT;
    stages[0].module = vertModule;
    stages[0].pName = "main";
    stages[1].sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
    stages[1].stage = VK_SHADER_STAGE_FRAGMENT_BIT;
    stages[1].module = fragModule;
    stages[1].pName = "main";

    VkVertexInputBindingDescription binding = {0};
    binding.binding = 0;
    binding.stride = sizeof(ImageVertex);
    binding.inputRate = VK_VERTEX_INPUT_RATE_VERTEX;

    VkVertexInputAttributeDescription attrs[2] = {0};
    attrs[0].location = 0;
    attrs[0].binding = 0;
    attrs[0].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[0].offset = offsetof(ImageVertex, pos);

    attrs[1].location = 1;
    attrs[1].binding = 0;
    attrs[1].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[1].offset = offsetof(ImageVertex, texCoord);

    VkPipelineVertexInputStateCreateInfo vertexInput = {0};
    vertexInput.sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO;
    vertexInput.vertexBindingDescriptionCount = 1;
    vertexInput.pVertexBindingDescriptions = &binding;
    vertexInput.vertexAttributeDescriptionCount = 2;
    vertexInput.pVertexAttributeDescriptions = attrs;

    VkPipelineInputAssemblyStateCreateInfo inputAssembly = {0};
    inputAssembly.sType = VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO;
    inputAssembly.topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;

    VkPipelineViewportStateCreateInfo viewport = {0};
    viewport.sType = VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO;
    viewport.viewportCount = 1;
    viewport.scissorCount = 1;

    VkPipelineRasterizationStateCreateInfo rasterizer = {0};
    rasterizer.sType = VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO;
    rasterizer.polygonMode = VK_POLYGON_MODE_FILL;
    rasterizer.lineWidth = 1.0f;
    rasterizer.cullMode = VK_CULL_MODE_NONE;
    rasterizer.frontFace = VK_FRONT_FACE_COUNTER_CLOCKWISE;

    VkPipelineMultisampleStateCreateInfo multisampling = {0};
    multisampling.sType = VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO;
    multisampling.rasterizationSamples = VK_SAMPLE_COUNT_1_BIT;

    VkPipelineDepthStencilStateCreateInfo depthStencil = {0};
    depthStencil.sType = VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO;
    depthStencil.depthTestEnable = VK_FALSE;
    depthStencil.depthWriteEnable = VK_FALSE;
    depthStencil.depthCompareOp = VK_COMPARE_OP_ALWAYS;
    depthStencil.depthBoundsTestEnable = VK_FALSE;
    depthStencil.stencilTestEnable = VK_FALSE;

    VkPipelineColorBlendAttachmentState blendAttachment = {0};
    blendAttachment.colorWriteMask = VK_COLOR_COMPONENT_R_BIT | VK_COLOR_COMPONENT_G_BIT |
                                     VK_COLOR_COMPONENT_B_BIT | VK_COLOR_COMPONENT_A_BIT;
    blendAttachment.blendEnable = VK_TRUE;
    blendAttachment.srcColorBlendFactor = VK_BLEND_FACTOR_SRC_ALPHA;
    blendAttachment.dstColorBlendFactor = VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA;
    blendAttachment.colorBlendOp = VK_BLEND_OP_ADD;
    blendAttachment.srcAlphaBlendFactor = VK_BLEND_FACTOR_ONE;
    blendAttachment.dstAlphaBlendFactor = VK_BLEND_FACTOR_ZERO;
    blendAttachment.alphaBlendOp = VK_BLEND_OP_ADD;

    VkPipelineColorBlendStateCreateInfo blending = {0};
    blending.sType = VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO;
    blending.attachmentCount = 1;
    blending.pAttachments = &blendAttachment;

    VkDynamicState dynamicStates[] = {VK_DYNAMIC_STATE_VIEWPORT, VK_DYNAMIC_STATE_SCISSOR};
    VkPipelineDynamicStateCreateInfo dynamicState = {0};
    dynamicState.sType = VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO;
    dynamicState.dynamicStateCount = 2;
    dynamicState.pDynamicStates = dynamicStates;

    VkPushConstantRange pushConstantRange = {0};
    pushConstantRange.stageFlags = VK_SHADER_STAGE_VERTEX_BIT;
    pushConstantRange.offset = 0;
    pushConstantRange.size = sizeof(float) * 2;

    VkPipelineLayoutCreateInfo layoutInfo = {0};
    layoutInfo.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO;
    layoutInfo.setLayoutCount = 1;
    layoutInfo.pSetLayouts = &ir->descriptorSetLayout;
    layoutInfo.pushConstantRangeCount = 1;
    layoutInfo.pPushConstantRanges = &pushConstantRange;

    vkCreatePipelineLayout(app->device, &layoutInfo, NULL, &ir->pipelineLayout);

    VkGraphicsPipelineCreateInfo pipelineInfo = {0};
    pipelineInfo.sType = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO;
    pipelineInfo.stageCount = 2;
    pipelineInfo.pStages = stages;
    pipelineInfo.pVertexInputState = &vertexInput;
    pipelineInfo.pInputAssemblyState = &inputAssembly;
    pipelineInfo.pViewportState = &viewport;
    pipelineInfo.pRasterizationState = &rasterizer;
    pipelineInfo.pMultisampleState = &multisampling;
    pipelineInfo.pDepthStencilState = &depthStencil;
    pipelineInfo.pColorBlendState = &blending;
    pipelineInfo.pDynamicState = &dynamicState;
    pipelineInfo.layout = ir->pipelineLayout;
    pipelineInfo.renderPass = app->renderPass;
    pipelineInfo.subpass = 0;

    vkCreateGraphicsPipelines(app->device, VK_NULL_HANDLE, 1, &pipelineInfo, NULL, &ir->pipeline);

    vkDestroyShaderModule(app->device, fragModule, NULL);
    vkDestroyShaderModule(app->device, vertModule, NULL);
    free(vertCode);
    free(fragCode);
}

void initImageRenderer(SiCompassApplication* app) {
    app->imageRenderer = (ImageRenderer*)calloc(1, sizeof(ImageRenderer));

    createImageVertexBuffer(app);
    createImageDescriptorSetLayout(app);
    createImageDescriptorPool(app);
    createImagePipeline(app);
}

static void destroyCachedTexture(SiCompassApplication* app, int index) {
    ImageRenderer* ir = app->imageRenderer;
    CachedTexture* ct = &ir->cache[index];
    if (!ct->loaded) return;

    vkDeviceWaitIdle(app->device);

    vkFreeDescriptorSets(app->device, ir->descriptorPool,
                         MAX_FRAMES_IN_FLIGHT, ct->descriptorSets);
    vkDestroySampler(app->device, ct->sampler, NULL);
    vkDestroyImageView(app->device, ct->imageView, NULL);
    vkDestroyImage(app->device, ct->image, NULL);
    vkFreeMemory(app->device, ct->memory, NULL);

    ct->loaded = false;
}

bool loadImageTexture(SiCompassApplication* app, const char* path) {
    ImageRenderer* ir = app->imageRenderer;

    // Check cache for existing entry
    for (int i = 0; i < MAX_CACHED_IMAGES; i++) {
        if (ir->cache[i].loaded && strcmp(ir->cache[i].path, path) == 0) {
            ir->cache[i].lastUsedFrame = ir->frameCounter;
            ir->lastLoadedCacheIndex = i;
            ir->textureWidth = ir->cache[i].width;
            ir->textureHeight = ir->cache[i].height;
            return true;
        }
    }

    // Find free slot or evict LRU
    int slot = -1;
    for (int i = 0; i < MAX_CACHED_IMAGES; i++) {
        if (!ir->cache[i].loaded) {
            slot = i;
            break;
        }
    }
    if (slot == -1) {
        uint32_t oldest = UINT32_MAX;
        for (int i = 0; i < MAX_CACHED_IMAGES; i++) {
            if (ir->cache[i].lastUsedFrame < oldest) {
                oldest = ir->cache[i].lastUsedFrame;
                slot = i;
            }
        }
        destroyCachedTexture(app, slot);
    }

    CachedTexture* ct = &ir->cache[slot];

    // Load image from disk
    int texWidth, texHeight, texChannels;
    stbi_uc* pixels = stbi_load(path, &texWidth, &texHeight, &texChannels, STBI_rgb_alpha);
    if (!pixels) {
        fprintf(stderr, "Failed to load image: %s\n", path);
        return false;
    }

    VkDeviceSize imageSize = texWidth * texHeight * 4;

    // Create staging buffer
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

    // Create Vulkan image
    createImage(app, texWidth, texHeight, VK_FORMAT_R8G8B8A8_SRGB, VK_IMAGE_TILING_OPTIMAL,
                VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT,
                &ct->image, &ct->memory);

    transitionImageLayout(app, ct->image, VK_FORMAT_R8G8B8A8_SRGB,
                         VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    copyBufferToImage(app, stagingBuffer, ct->image, (uint32_t)texWidth, (uint32_t)texHeight);
    transitionImageLayout(app, ct->image, VK_FORMAT_R8G8B8A8_SRGB,
                         VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);

    vkDestroyBuffer(app->device, stagingBuffer, NULL);
    vkFreeMemory(app->device, stagingBufferMemory, NULL);

    // Create image view
    ct->imageView = createImageView(app, ct->image, VK_FORMAT_R8G8B8A8_SRGB, VK_IMAGE_ASPECT_COLOR_BIT);

    // Create sampler
    VkSamplerCreateInfo samplerInfo = {0};
    samplerInfo.sType = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO;
    samplerInfo.magFilter = VK_FILTER_LINEAR;
    samplerInfo.minFilter = VK_FILTER_LINEAR;
    samplerInfo.addressModeU = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
    samplerInfo.addressModeV = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
    samplerInfo.addressModeW = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
    samplerInfo.anisotropyEnable = VK_FALSE;
    samplerInfo.borderColor = VK_BORDER_COLOR_INT_OPAQUE_BLACK;
    samplerInfo.unnormalizedCoordinates = VK_FALSE;
    samplerInfo.compareEnable = VK_FALSE;
    samplerInfo.mipmapMode = VK_SAMPLER_MIPMAP_MODE_LINEAR;

    if (vkCreateSampler(app->device, &samplerInfo, NULL, &ct->sampler) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create image texture sampler!\n");
        return false;
    }

    // Allocate descriptor sets for this cache entry
    VkDescriptorSetLayout layouts[MAX_FRAMES_IN_FLIGHT];
    for (int i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        layouts[i] = ir->descriptorSetLayout;
    }

    VkDescriptorSetAllocateInfo allocInfo = {0};
    allocInfo.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO;
    allocInfo.descriptorPool = ir->descriptorPool;
    allocInfo.descriptorSetCount = (uint32_t)MAX_FRAMES_IN_FLIGHT;
    allocInfo.pSetLayouts = layouts;

    if (vkAllocateDescriptorSets(app->device, &allocInfo, ct->descriptorSets) != VK_SUCCESS) {
        fprintf(stderr, "Failed to allocate image descriptor sets!\n");
        return false;
    }

    for (size_t i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        VkDescriptorImageInfo imageInfo = {0};
        imageInfo.imageLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
        imageInfo.imageView = ct->imageView;
        imageInfo.sampler = ct->sampler;

        VkWriteDescriptorSet descriptorWrite = {0};
        descriptorWrite.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;
        descriptorWrite.dstSet = ct->descriptorSets[i];
        descriptorWrite.dstBinding = 0;
        descriptorWrite.descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
        descriptorWrite.descriptorCount = 1;
        descriptorWrite.pImageInfo = &imageInfo;

        vkUpdateDescriptorSets(app->device, 1, &descriptorWrite, 0, NULL);
    }

    ct->width = texWidth;
    ct->height = texHeight;
    ct->loaded = true;
    ct->lastUsedFrame = ir->frameCounter;
    strncpy(ct->path, path, sizeof(ct->path) - 1);
    ct->path[sizeof(ct->path) - 1] = '\0';

    ir->lastLoadedCacheIndex = slot;
    ir->textureWidth = texWidth;
    ir->textureHeight = texHeight;

    return true;
}

void beginImageRendering(SiCompassApplication* app) {
    ImageRenderer* ir = app->imageRenderer;
    ir->drawCallCount = 0;
    ir->frameCounter++;
}

void prepareImage(SiCompassApplication* app, float x, float y, float width, float height) {
    ImageRenderer* ir = app->imageRenderer;
    if (ir->drawCallCount >= MAX_VISIBLE_IMAGES) return;

    ImageDrawCall* dc = &ir->drawCalls[ir->drawCallCount];
    dc->cacheIndex = ir->lastLoadedCacheIndex;

    float minX = x;
    float minY = y;
    float maxX = x + width;
    float maxY = y + height;

    // Triangle 1
    dc->vertices[0] = (ImageVertex){{minX, minY}, {0.0f, 0.0f}};
    dc->vertices[1] = (ImageVertex){{maxX, minY}, {1.0f, 0.0f}};
    dc->vertices[2] = (ImageVertex){{maxX, maxY}, {1.0f, 1.0f}};

    // Triangle 2
    dc->vertices[3] = (ImageVertex){{minX, minY}, {0.0f, 0.0f}};
    dc->vertices[4] = (ImageVertex){{maxX, maxY}, {1.0f, 1.0f}};
    dc->vertices[5] = (ImageVertex){{minX, maxY}, {0.0f, 1.0f}};

    ir->drawCallCount++;
}

void drawImageQuad(SiCompassApplication* app, VkCommandBuffer commandBuffer) {
    ImageRenderer* ir = app->imageRenderer;
    if (ir->drawCallCount == 0) return;

    // Upload all vertices at once
    VkDeviceSize totalSize = ir->drawCallCount * 6 * sizeof(ImageVertex);
    void* data;
    vkMapMemory(app->device, ir->vertexBufferMemory, 0, totalSize, 0, &data);
    for (uint32_t i = 0; i < ir->drawCallCount; i++) {
        memcpy((char*)data + i * 6 * sizeof(ImageVertex),
               ir->drawCalls[i].vertices, 6 * sizeof(ImageVertex));
    }
    vkUnmapMemory(app->device, ir->vertexBufferMemory);

    float screenDimensions[2] = {
        (float)app->swapChainExtent.width,
        (float)app->swapChainExtent.height
    };

    vkCmdBindPipeline(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS, ir->pipeline);

    vkCmdPushConstants(commandBuffer, ir->pipelineLayout,
                      VK_SHADER_STAGE_VERTEX_BIT, 0, sizeof(screenDimensions),
                      screenDimensions);

    VkBuffer buffers[] = {ir->vertexBuffer};
    VkDeviceSize offsets[] = {0};
    vkCmdBindVertexBuffers(commandBuffer, 0, 1, buffers, offsets);

    for (uint32_t i = 0; i < ir->drawCallCount; i++) {
        CachedTexture* ct = &ir->cache[ir->drawCalls[i].cacheIndex];

        vkCmdBindDescriptorSets(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS,
                               ir->pipelineLayout, 0, 1,
                               &ct->descriptorSets[app->currentFrame],
                               0, NULL);

        vkCmdDraw(commandBuffer, 6, 1, i * 6, 0);
    }
}

void cleanupImageRenderer(SiCompassApplication* app) {
    ImageRenderer* ir = app->imageRenderer;
    if (!ir) return;

    for (int i = 0; i < MAX_CACHED_IMAGES; i++) {
        destroyCachedTexture(app, i);
    }

    vkDestroyPipeline(app->device, ir->pipeline, NULL);
    vkDestroyPipelineLayout(app->device, ir->pipelineLayout, NULL);
    vkDestroyDescriptorPool(app->device, ir->descriptorPool, NULL);
    vkDestroyDescriptorSetLayout(app->device, ir->descriptorSetLayout, NULL);
    vkDestroyBuffer(app->device, ir->vertexBuffer, NULL);
    vkFreeMemory(app->device, ir->vertexBufferMemory, NULL);

    free(ir);
    app->imageRenderer = NULL;
}

// ============================================================================
// GENERAL VULKAN SETUP FUNCTIONS (not image-specific)
// ============================================================================

void createRenderPass(SiCompassApplication* app) {
    VkAttachmentDescription colorAttachment = {0};
    colorAttachment.format = app->swapChainImageFormat;
    colorAttachment.samples = VK_SAMPLE_COUNT_1_BIT;
    colorAttachment.loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR;
    colorAttachment.storeOp = VK_ATTACHMENT_STORE_OP_STORE;
    colorAttachment.stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE;
    colorAttachment.stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    colorAttachment.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    colorAttachment.finalLayout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;

    VkAttachmentDescription depthAttachment = {0};
    depthAttachment.format = findDepthFormat(app);
    depthAttachment.samples = VK_SAMPLE_COUNT_1_BIT;
    depthAttachment.loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR;
    depthAttachment.storeOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    depthAttachment.stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE;
    depthAttachment.stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE;
    depthAttachment.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    depthAttachment.finalLayout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL;

    VkAttachmentReference colorAttachmentRef = {0};
    colorAttachmentRef.attachment = 0;
    colorAttachmentRef.layout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;

    VkAttachmentReference depthAttachmentRef = {0};
    depthAttachmentRef.attachment = 1;
    depthAttachmentRef.layout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL;

    VkSubpassDescription subpass = {0};
    subpass.pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS;
    subpass.colorAttachmentCount = 1;
    subpass.pColorAttachments = &colorAttachmentRef;
    subpass.pDepthStencilAttachment = &depthAttachmentRef;

    VkSubpassDependency dependency = {0};
    dependency.srcSubpass = VK_SUBPASS_EXTERNAL;
    dependency.dstSubpass = 0;
    dependency.srcStageMask = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT;
    dependency.srcAccessMask = VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT;
    dependency.dstStageMask = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT;
    dependency.dstAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT | VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT;

    VkAttachmentDescription attachments[2] = {colorAttachment, depthAttachment};
    VkRenderPassCreateInfo renderPassInfo = {0};
    renderPassInfo.sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO;
    renderPassInfo.attachmentCount = 2;
    renderPassInfo.pAttachments = attachments;
    renderPassInfo.subpassCount = 1;
    renderPassInfo.pSubpasses = &subpass;
    renderPassInfo.dependencyCount = 1;
    renderPassInfo.pDependencies = &dependency;

    if (vkCreateRenderPass(app->device, &renderPassInfo, NULL, &app->renderPass) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create render pass!\n");
        exit(EXIT_FAILURE);
    }
}

void createFramebuffers(SiCompassApplication* app) {
    app->swapChainFramebuffers = malloc(sizeof(VkFramebuffer) * app->swapChainImageCount);

    for (uint32_t i = 0; i < app->swapChainImageCount; i++) {
        VkImageView attachments[2] = {
            app->swapChainImageViews[i],
            app->depthImageView
        };

        VkFramebufferCreateInfo framebufferInfo = {0};
        framebufferInfo.sType = VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO;
        framebufferInfo.renderPass = app->renderPass;
        framebufferInfo.attachmentCount = 2;
        framebufferInfo.pAttachments = attachments;
        framebufferInfo.width = app->swapChainExtent.width;
        framebufferInfo.height = app->swapChainExtent.height;
        framebufferInfo.layers = 1;

        if (vkCreateFramebuffer(app->device, &framebufferInfo, NULL, &app->swapChainFramebuffers[i]) != VK_SUCCESS) {
            fprintf(stderr, "Failed to create framebuffer!\n");
            exit(EXIT_FAILURE);
        }
    }
}

void createCommandPool(SiCompassApplication* app) {
    QueueFamilyIndices queueFamilyIndices = findQueueFamilies(app->physicalDevice, app->surface);

    VkCommandPoolCreateInfo poolInfo = {0};
    poolInfo.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
    poolInfo.queueFamilyIndex = queueFamilyIndices.graphicsFamily;

    if (vkCreateCommandPool(app->device, &poolInfo, NULL, &app->commandPool) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create command pool!\n");
        exit(EXIT_FAILURE);
    }
}

VkFormat findSupportedFormat(SiCompassApplication* app, const VkFormat* candidates, size_t candidateCount,
                             VkImageTiling tiling, VkFormatFeatureFlags features) {
    for (size_t i = 0; i < candidateCount; i++) {
        VkFormat format = candidates[i];
        VkFormatProperties props;
        vkGetPhysicalDeviceFormatProperties(app->physicalDevice, format, &props);

        if (tiling == VK_IMAGE_TILING_LINEAR && (props.linearTilingFeatures & features) == features) {
            return format;
        } else if (tiling == VK_IMAGE_TILING_OPTIMAL && (props.optimalTilingFeatures & features) == features) {
            return format;
        }
    }

    fprintf(stderr, "Failed to find supported format!\n");
    return VK_FORMAT_UNDEFINED;
}

VkFormat findDepthFormat(SiCompassApplication* app) {
    VkFormat candidates[] = {
        VK_FORMAT_D32_SFLOAT,
        VK_FORMAT_D32_SFLOAT_S8_UINT,
        VK_FORMAT_D24_UNORM_S8_UINT
    };

    return findSupportedFormat(
        app,
        candidates,
        sizeof(candidates) / sizeof(candidates[0]),
        VK_IMAGE_TILING_OPTIMAL,
        VK_FORMAT_FEATURE_DEPTH_STENCIL_ATTACHMENT_BIT
    );
}

void createDepthResources(SiCompassApplication* app) {
    VkFormat depthFormat = findDepthFormat(app);

    createImage(app, app->swapChainExtent.width, app->swapChainExtent.height, depthFormat,
                VK_IMAGE_TILING_OPTIMAL, VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT,
                VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT, &app->depthImage, &app->depthImageMemory);
    app->depthImageView = createImageView(app, app->depthImage, depthFormat, VK_IMAGE_ASPECT_DEPTH_BIT);
}

void createCommandBuffers(SiCompassApplication* app) {
    VkCommandBufferAllocateInfo allocInfo = {0};
    allocInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocInfo.commandPool = app->commandPool;
    allocInfo.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocInfo.commandBufferCount = (uint32_t)MAX_FRAMES_IN_FLIGHT;

    if (vkAllocateCommandBuffers(app->device, &allocInfo, app->commandBuffers) != VK_SUCCESS) {
        fprintf(stderr, "Failed to allocate command buffers!\n");
        exit(EXIT_FAILURE);
    }
}

void createSyncObjects(SiCompassApplication* app) {
    VkSemaphoreCreateInfo semaphoreInfo = {0};
    semaphoreInfo.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;

    VkFenceCreateInfo fenceInfo = {0};
    fenceInfo.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
    fenceInfo.flags = VK_FENCE_CREATE_SIGNALED_BIT;

    for (size_t i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        if (vkCreateSemaphore(app->device, &semaphoreInfo, NULL, &app->imageAvailableSemaphores[i]) != VK_SUCCESS ||
            vkCreateSemaphore(app->device, &semaphoreInfo, NULL, &app->renderFinishedSemaphores[i]) != VK_SUCCESS ||
            vkCreateFence(app->device, &fenceInfo, NULL, &app->inFlightFences[i]) != VK_SUCCESS) {
            fprintf(stderr, "Failed to create synchronization objects for a frame!\n");
            exit(EXIT_FAILURE);
        }
    }
}
