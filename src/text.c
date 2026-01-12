#include "text.h"
#include "main.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

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
extern VkShaderModule createShaderModule(VkDevice device, const char* code, size_t codeSize);
extern char* readFile(const char* filename, size_t* outSize);

// ============================================================================
// FONT RENDERING FUNCTIONS
// ============================================================================

void initFreeType(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    if (FT_Init_FreeType(&fr->ftLibrary)) {
        fprintf(stderr, "Could not init FreeType Library\n");
        exit(EXIT_FAILURE);
    }

    const char* fontPath = "fonts/Consolas-Regular.ttf";
    FT_Error error = FT_New_Face(fr->ftLibrary, fontPath, 0, &fr->ftFace);
    if (error) {
        fprintf(stderr, "Failed to load font: %s (error code: %d)\n", fontPath, error);
        exit(EXIT_FAILURE);
    }

    // SDL3: Use display content scale and assume 96 DPI base
    SDL_DisplayID displayID = SDL_GetDisplayForWindow(app->window);
    float contentScale = SDL_GetDisplayContentScale(displayID);

    // Base DPI of 96, scaled by content scale
    int scaledDPI = (int)(96.0f * contentScale);
    // Use larger base size (48pt) for better quality when scaling
    FT_Set_Char_Size(fr->ftFace, 0, 48*64, scaledDPI, scaledDPI);

    // Store font metrics for consistent line height
    fr->ascender = fr->ftFace->size->metrics.ascender / 64.0f;
    fr->descender = fr->ftFace->size->metrics.descender / 64.0f;
    fr->lineHeight = fr->ascender - fr->descender;

}

void createFontAtlas(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    unsigned char* atlasData = (unsigned char*)calloc(FONT_ATLAS_SIZE * FONT_ATLAS_SIZE, 1);

    int penX = 0, penY = 0, rowHeight = 0;

    for (unsigned char c = 32; c < 128; c++) {
        if (FT_Load_Char(fr->ftFace, c, FT_LOAD_RENDER)) continue;

        FT_GlyphSlot g = fr->ftFace->glyph;

        if (penX + g->bitmap.width >= FONT_ATLAS_SIZE) {
            penX = 0;
            penY += rowHeight;
            rowHeight = 0;
        }

        for (uint32_t row = 0; row < g->bitmap.rows; row++) {
            for (uint32_t col = 0; col < g->bitmap.width; col++) {
                int x = penX + col;
                int y = penY + row;
                if (x < FONT_ATLAS_SIZE && y < FONT_ATLAS_SIZE) {
                    atlasData[y * FONT_ATLAS_SIZE + x] =
                        g->bitmap.buffer[row * g->bitmap.width + col];
                }
            }
        }

        fr->glyphs[c].size[0] = (float)g->bitmap.width;
        fr->glyphs[c].size[1] = (float)g->bitmap.rows;
        fr->glyphs[c].bearing[0] = (float)g->bitmap_left;
        fr->glyphs[c].bearing[1] = (float)g->bitmap_top;
        fr->glyphs[c].advance = (uint32_t)(g->advance.x >> 6);
        fr->glyphs[c].uvMin[0] = (float)penX / FONT_ATLAS_SIZE;
        fr->glyphs[c].uvMin[1] = (float)penY / FONT_ATLAS_SIZE;
        fr->glyphs[c].uvMax[0] = (float)(penX + g->bitmap.width) / FONT_ATLAS_SIZE;
        fr->glyphs[c].uvMax[1] = (float)(penY + g->bitmap.rows) / FONT_ATLAS_SIZE;

        penX += g->bitmap.width + 1;
        rowHeight = (g->bitmap.rows > rowHeight) ? g->bitmap.rows : rowHeight;
    }

    VkDeviceSize imageSize = FONT_ATLAS_SIZE * FONT_ATLAS_SIZE;

    VkBuffer stagingBuffer;
    VkDeviceMemory stagingBufferMemory;
    createBuffer(app, imageSize, VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                 &stagingBuffer, &stagingBufferMemory);

    void* data;
    vkMapMemory(app->device, stagingBufferMemory, 0, imageSize, 0, &data);
    memcpy(data, atlasData, imageSize);
    vkUnmapMemory(app->device, stagingBufferMemory);
    free(atlasData);

    createImage(app, FONT_ATLAS_SIZE, FONT_ATLAS_SIZE, VK_FORMAT_R8_UNORM,
                VK_IMAGE_TILING_OPTIMAL,
                VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT,
                VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT,
                &fr->fontAtlasImage, &fr->fontAtlasMemory);

    transitionImageLayout(app, fr->fontAtlasImage, VK_FORMAT_R8_UNORM,
                         VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
    copyBufferToImage(app, stagingBuffer, fr->fontAtlasImage,
                     FONT_ATLAS_SIZE, FONT_ATLAS_SIZE);
    transitionImageLayout(app, fr->fontAtlasImage, VK_FORMAT_R8_UNORM,
                         VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                         VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);

    vkDestroyBuffer(app->device, stagingBuffer, NULL);
    vkFreeMemory(app->device, stagingBufferMemory, NULL);
}

void createFontAtlasView(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    VkImageViewCreateInfo viewInfo = {0};
    viewInfo.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    viewInfo.image = fr->fontAtlasImage;
    viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    viewInfo.format = VK_FORMAT_R8_UNORM;
    viewInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    viewInfo.subresourceRange.baseMipLevel = 0;
    viewInfo.subresourceRange.levelCount = 1;
    viewInfo.subresourceRange.baseArrayLayer = 0;
    viewInfo.subresourceRange.layerCount = 1;

    if (vkCreateImageView(app->device, &viewInfo, NULL, &fr->fontAtlasView) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create font atlas image view!\n");
        exit(EXIT_FAILURE);
    }
}

void createFontAtlasSampler(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

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

    if (vkCreateSampler(app->device, &samplerInfo, NULL, &fr->fontAtlasSampler) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create font atlas sampler!\n");
        exit(EXIT_FAILURE);
    }
}

void createTextVertexBuffer(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;
    VkDeviceSize bufferSize = sizeof(TextVertex) * MAX_TEXT_VERTICES;

    createBuffer(app, bufferSize, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                 &fr->textVertexBuffer, &fr->textVertexBufferMemory);
}

void createBackgroundVertexBuffer(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;
    VkDeviceSize bufferSize = sizeof(BackgroundVertex) * 6;

    createBuffer(app, bufferSize, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                 &fr->backgroundVertexBuffer, &fr->backgroundVertexBufferMemory);
}

void createTextDescriptorSetLayout(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

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
                                    &fr->textDescriptorSetLayout) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create text descriptor set layout!\n");
        exit(EXIT_FAILURE);
    }
}

void createTextDescriptorPool(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    VkDescriptorPoolSize poolSize = {0};
    poolSize.type = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
    poolSize.descriptorCount = (uint32_t)MAX_FRAMES_IN_FLIGHT;

    VkDescriptorPoolCreateInfo poolInfo = {0};
    poolInfo.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO;
    poolInfo.poolSizeCount = 1;
    poolInfo.pPoolSizes = &poolSize;
    poolInfo.maxSets = (uint32_t)MAX_FRAMES_IN_FLIGHT;

    if (vkCreateDescriptorPool(app->device, &poolInfo, NULL,
                               &fr->textDescriptorPool) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create text descriptor pool!\n");
        exit(EXIT_FAILURE);
    }
}

void createTextDescriptorSets(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    VkDescriptorSetLayout layouts[MAX_FRAMES_IN_FLIGHT];
    for (int i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        layouts[i] = fr->textDescriptorSetLayout;
    }

    VkDescriptorSetAllocateInfo allocInfo = {0};
    allocInfo.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO;
    allocInfo.descriptorPool = fr->textDescriptorPool;
    allocInfo.descriptorSetCount = (uint32_t)MAX_FRAMES_IN_FLIGHT;
    allocInfo.pSetLayouts = layouts;

    if (vkAllocateDescriptorSets(app->device, &allocInfo,
                                 fr->textDescriptorSets) != VK_SUCCESS) {
        fprintf(stderr, "Failed to allocate text descriptor sets!\n");
        exit(EXIT_FAILURE);
    }

    for (size_t i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        VkDescriptorImageInfo imageInfo = {0};
        imageInfo.imageLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
        imageInfo.imageView = fr->fontAtlasView;
        imageInfo.sampler = fr->fontAtlasSampler;

        VkWriteDescriptorSet descriptorWrite = {0};
        descriptorWrite.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;
        descriptorWrite.dstSet = fr->textDescriptorSets[i];
        descriptorWrite.dstBinding = 0;
        descriptorWrite.descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
        descriptorWrite.descriptorCount = 1;
        descriptorWrite.pImageInfo = &imageInfo;

        vkUpdateDescriptorSets(app->device, 1, &descriptorWrite, 0, NULL);
    }
}

void createTextPipeline(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    size_t vertSize, fragSize;
    char* vertCode = readFile("shaders/text_vert.spv", &vertSize);
    char* fragCode = readFile("shaders/text_frag.spv", &fragSize);

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
    binding.stride = sizeof(TextVertex);
    binding.inputRate = VK_VERTEX_INPUT_RATE_VERTEX;

    VkVertexInputAttributeDescription attrs[3] = {0};
    attrs[0].location = 0;
    attrs[0].binding = 0;
    attrs[0].format = VK_FORMAT_R32G32B32_SFLOAT;
    attrs[0].offset = offsetof(TextVertex, pos);

    attrs[1].location = 1;
    attrs[1].binding = 0;
    attrs[1].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[1].offset = offsetof(TextVertex, texCoord);

    attrs[2].location = 2;
    attrs[2].binding = 0;
    attrs[2].format = VK_FORMAT_R32G32B32_SFLOAT;
    attrs[2].offset = offsetof(TextVertex, color);

    VkPipelineVertexInputStateCreateInfo vertexInput = {0};
    vertexInput.sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO;
    vertexInput.vertexBindingDescriptionCount = 1;
    vertexInput.pVertexBindingDescriptions = &binding;
    vertexInput.vertexAttributeDescriptionCount = 3;
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
    layoutInfo.pSetLayouts = &fr->textDescriptorSetLayout;
    layoutInfo.pushConstantRangeCount = 1;
    layoutInfo.pPushConstantRanges = &pushConstantRange;

    vkCreatePipelineLayout(app->device, &layoutInfo, NULL, &fr->textPipelineLayout);

    VkGraphicsPipelineCreateInfo pipelineInfo = {0};
    pipelineInfo.sType = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO;
    pipelineInfo.stageCount = 2;
    pipelineInfo.pStages = stages;
    pipelineInfo.pVertexInputState = &vertexInput;
    pipelineInfo.pInputAssemblyState = &inputAssembly;
    pipelineInfo.pViewportState = &viewport;
    pipelineInfo.pRasterizationState = &rasterizer;
    pipelineInfo.pMultisampleState = &multisampling;
    pipelineInfo.pColorBlendState = &blending;
    pipelineInfo.pDynamicState = &dynamicState;
    pipelineInfo.layout = fr->textPipelineLayout;
    pipelineInfo.renderPass = app->renderPass;
    pipelineInfo.subpass = 0;

    vkCreateGraphicsPipelines(app->device, VK_NULL_HANDLE, 1, &pipelineInfo, NULL, &fr->textPipeline);

    vkDestroyShaderModule(app->device, fragModule, NULL);
    vkDestroyShaderModule(app->device, vertModule, NULL);
    free(vertCode);
    free(fragCode);
}

void createBackgroundPipeline(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    size_t vertSize, fragSize;
    char* vertCode = readFile("shaders/background_vert.spv", &vertSize);
    char* fragCode = readFile("shaders/background_frag.spv", &fragSize);

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
    binding.stride = sizeof(BackgroundVertex);
    binding.inputRate = VK_VERTEX_INPUT_RATE_VERTEX;

    VkVertexInputAttributeDescription attrs[5] = {0};
    attrs[0].location = 0;
    attrs[0].binding = 0;
    attrs[0].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[0].offset = offsetof(BackgroundVertex, pos);

    attrs[1].location = 1;
    attrs[1].binding = 0;
    attrs[1].format = VK_FORMAT_R32G32B32A32_SFLOAT;
    attrs[1].offset = offsetof(BackgroundVertex, color);

    attrs[2].location = 2;
    attrs[2].binding = 0;
    attrs[2].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[2].offset = offsetof(BackgroundVertex, cornerRadius);

    attrs[3].location = 3;
    attrs[3].binding = 0;
    attrs[3].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[3].offset = offsetof(BackgroundVertex, rectSize);

    attrs[4].location = 4;
    attrs[4].binding = 0;
    attrs[4].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[4].offset = offsetof(BackgroundVertex, rectOrigin);

    VkPipelineVertexInputStateCreateInfo vertexInput = {0};
    vertexInput.sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO;
    vertexInput.vertexBindingDescriptionCount = 1;
    vertexInput.pVertexBindingDescriptions = &binding;
    vertexInput.vertexAttributeDescriptionCount = 5;
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
    layoutInfo.setLayoutCount = 0;
    layoutInfo.pSetLayouts = NULL;
    layoutInfo.pushConstantRangeCount = 1;
    layoutInfo.pPushConstantRanges = &pushConstantRange;

    vkCreatePipelineLayout(app->device, &layoutInfo, NULL, &fr->backgroundPipelineLayout);

    VkGraphicsPipelineCreateInfo pipelineInfo = {0};
    pipelineInfo.sType = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO;
    pipelineInfo.stageCount = 2;
    pipelineInfo.pStages = stages;
    pipelineInfo.pVertexInputState = &vertexInput;
    pipelineInfo.pInputAssemblyState = &inputAssembly;
    pipelineInfo.pViewportState = &viewport;
    pipelineInfo.pRasterizationState = &rasterizer;
    pipelineInfo.pMultisampleState = &multisampling;
    pipelineInfo.pColorBlendState = &blending;
    pipelineInfo.pDynamicState = &dynamicState;
    pipelineInfo.layout = fr->backgroundPipelineLayout;
    pipelineInfo.renderPass = app->renderPass;
    pipelineInfo.subpass = 0;

    vkCreateGraphicsPipelines(app->device, VK_NULL_HANDLE, 1, &pipelineInfo, NULL, &fr->backgroundPipeline);

    vkDestroyShaderModule(app->device, fragModule, NULL);
    vkDestroyShaderModule(app->device, vertModule, NULL);
    free(vertCode);
    free(fragCode);
}

void calculateTextBounds(SiCompassApplication* app, const char* text,
                        float x, float y, float scale,
                        float* outMinX, float* outMinY,
                        float* outMaxX, float* outMaxY) {
    FontRenderer* fr = app->fontRenderer;

    float cursorX = x;
    float minX = x;
    float maxX = x;
    bool first = true;

    // Use consistent line height based on font metrics
    float minY = y - fr->ascender * scale;
    float maxY = y - fr->descender * scale;

    for (const char* p = text; *p; p++) {
        char c = *p;
        if (c < 32 || c >= 128) continue;

        GlyphInfo* g = &fr->glyphs[(int)c];

        float xpos = cursorX + g->bearing[0] * scale;
        float w = g->size[0] * scale;

        if (first) {
            minX = xpos;
            maxX = xpos + w;
            first = false;
        } else {
            if (xpos < minX) minX = xpos;
            if (xpos + w > maxX) maxX = xpos + w;
        }

        cursorX += g->advance * scale;
    }

    *outMinX = minX;
    *outMinY = minY;
    *outMaxX = maxX;
    *outMaxY = maxY;
}

float getWidthEM(SiCompassApplication* app, float scale) {
    FontRenderer* fr = app->fontRenderer;

    // Get the 'M' character glyph info
    GlyphInfo* g = &fr->glyphs[(int)'M'];

    // Return the advance width (horizontal spacing) scaled
    return g->advance * scale;
}

float getLineHeight(SiCompassApplication* app, float scale, float padding) {
    FontRenderer* fr = app->fontRenderer;

    // Calculate line height from font metrics (ascender - descender)
    // Add padding for top and bottom margins
    return fr->lineHeight * scale + (padding * 2.0f);
}

void prepareBackgroundForText(SiCompassApplication* app, const char* text,
                              float x, float y, float scale,
                              vec4 bgColor, float cornerRadius, float padding) {
    FontRenderer* fr = app->fontRenderer;

    // Calculate text bounds
    float minX, minY, maxX, maxY;
    calculateTextBounds(app, text, x, y, scale, &minX, &minY, &maxX, &maxY);

    // Add padding
    minX -= padding;
    minY -= padding;
    maxX += padding;
    maxY += padding;

    float width = maxX - minX;
    float height = maxY - minY;

    // Clamp corner radius to prevent it from being larger than the rectangle
    float maxRadius = fminf(width, height) * 0.5f;
    float actualCornerRadius = fminf(cornerRadius, maxRadius);

    // Create 6 vertices for 2 triangles (a quad)
    BackgroundVertex vertices[6];

    // Bottom-left corner is our reference point
    for (int i = 0; i < 6; i++) {
        vertices[i].color[0] = bgColor[0];
        vertices[i].color[1] = bgColor[1];
        vertices[i].color[2] = bgColor[2];
        vertices[i].color[3] = bgColor[3];
        vertices[i].cornerRadius[0] = actualCornerRadius;
        vertices[i].cornerRadius[1] = 0.0f;
        vertices[i].rectSize[0] = width;
        vertices[i].rectSize[1] = height;
        vertices[i].rectOrigin[0] = minX;
        vertices[i].rectOrigin[1] = minY;
    }

    // Triangle 1
    vertices[0].pos[0] = minX; vertices[0].pos[1] = minY;
    vertices[1].pos[0] = maxX; vertices[1].pos[1] = minY;
    vertices[2].pos[0] = maxX; vertices[2].pos[1] = maxY;

    // Triangle 2
    vertices[3].pos[0] = minX; vertices[3].pos[1] = minY;
    vertices[4].pos[0] = maxX; vertices[4].pos[1] = maxY;
    vertices[5].pos[0] = minX; vertices[5].pos[1] = maxY;

    fr->backgroundVertexCount = 6;

    // Upload to GPU
    void* data;
    vkMapMemory(app->device, fr->backgroundVertexBufferMemory, 0,
                sizeof(BackgroundVertex) * 6, 0, &data);
    memcpy(data, vertices, sizeof(BackgroundVertex) * 6);
    vkUnmapMemory(app->device, fr->backgroundVertexBufferMemory);
}

void beginTextRendering(SiCompassApplication* app) {
    app->fontRenderer->textVertexCount = 0;
}

void prepareTextForRendering(SiCompassApplication* app, const char* text,
                             float x, float y, float scale, vec3 color) {
    FontRenderer* fr = app->fontRenderer;

    TextVertex vertices[MAX_TEXT_VERTICES];
    uint32_t vi = 0;

    float cursorX = x;
    float cursorY = y;

    for (const char* p = text; *p; p++) {
        char c = *p;
        if (c < 32 || c >= 128) continue;

        GlyphInfo* g = &fr->glyphs[(int)c];

        float xpos = cursorX + g->bearing[0] * scale;
        float ypos = cursorY - g->bearing[1] * scale;
        float w = g->size[0] * scale;
        float h = g->size[1] * scale;

        if (vi + 6 > MAX_TEXT_VERTICES) break;

        vertices[vi++] = (TextVertex){{xpos, ypos + h, 0.0f}, {g->uvMin[0], g->uvMax[1]}, {color[0], color[1], color[2]}};
        vertices[vi++] = (TextVertex){{xpos, ypos, 0.0f}, {g->uvMin[0], g->uvMin[1]}, {color[0], color[1], color[2]}};
        vertices[vi++] = (TextVertex){{xpos + w, ypos, 0.0f}, {g->uvMax[0], g->uvMin[1]}, {color[0], color[1], color[2]}};

        vertices[vi++] = (TextVertex){{xpos, ypos + h, 0.0f}, {g->uvMin[0], g->uvMax[1]}, {color[0], color[1], color[2]}};
        vertices[vi++] = (TextVertex){{xpos + w, ypos, 0.0f}, {g->uvMax[0], g->uvMin[1]}, {color[0], color[1], color[2]}};
        vertices[vi++] = (TextVertex){{xpos + w, ypos + h, 0.0f}, {g->uvMax[0], g->uvMax[1]}, {color[0], color[1], color[2]}};

        cursorX += g->advance * scale;
    }

    // Check if adding these vertices would exceed the buffer size
    if (fr->textVertexCount + vi > MAX_TEXT_VERTICES) {
        return; // Skip this text if it doesn't fit
    }

    // Append vertices to the buffer at the current offset
    void* data;
    vkMapMemory(app->device, fr->textVertexBufferMemory,
                sizeof(TextVertex) * fr->textVertexCount,
                sizeof(TextVertex) * vi, 0, &data);
    memcpy(data, vertices, sizeof(TextVertex) * vi);
    vkUnmapMemory(app->device, fr->textVertexBufferMemory);

    // Increment the vertex count
    fr->textVertexCount += vi;
}

void cleanupFontRenderer(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    vkDestroyPipeline(app->device, fr->backgroundPipeline, NULL);
    vkDestroyPipelineLayout(app->device, fr->backgroundPipelineLayout, NULL);
    vkDestroyBuffer(app->device, fr->backgroundVertexBuffer, NULL);
    vkFreeMemory(app->device, fr->backgroundVertexBufferMemory, NULL);

    vkDestroyPipeline(app->device, fr->textPipeline, NULL);
    vkDestroyPipelineLayout(app->device, fr->textPipelineLayout, NULL);
    vkDestroyDescriptorPool(app->device, fr->textDescriptorPool, NULL);
    vkDestroyDescriptorSetLayout(app->device, fr->textDescriptorSetLayout, NULL);
    vkDestroyBuffer(app->device, fr->textVertexBuffer, NULL);
    vkFreeMemory(app->device, fr->textVertexBufferMemory, NULL);
    vkDestroySampler(app->device, fr->fontAtlasSampler, NULL);
    vkDestroyImageView(app->device, fr->fontAtlasView, NULL);
    vkDestroyImage(app->device, fr->fontAtlasImage, NULL);
    vkFreeMemory(app->device, fr->fontAtlasMemory, NULL);

    FT_Done_Face(fr->ftFace);
    FT_Done_FreeType(fr->ftLibrary);

    free(app->fontRenderer);
}

void drawBackground(SiCompassApplication* app, VkCommandBuffer commandBuffer) {
    FontRenderer* fr = app->fontRenderer;

    // Push screen dimensions for background pipeline
    float screenDimensions[2] = {
        (float)app->swapChainExtent.width,
        (float)app->swapChainExtent.height
    };

    // Draw background
    if (fr->backgroundVertexCount > 0) {
        vkCmdBindPipeline(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS,
                         fr->backgroundPipeline);

        vkCmdPushConstants(commandBuffer, fr->backgroundPipelineLayout,
                          VK_SHADER_STAGE_VERTEX_BIT, 0, sizeof(screenDimensions),
                          screenDimensions);

        VkBuffer backgroundBuffers[] = {fr->backgroundVertexBuffer};
        VkDeviceSize offsets[] = {0};
        vkCmdBindVertexBuffers(commandBuffer, 0, 1, backgroundBuffers, offsets);

        vkCmdDraw(commandBuffer, fr->backgroundVertexCount, 1, 0, 0);
    }
}

void drawText(SiCompassApplication* app, VkCommandBuffer commandBuffer) {
    FontRenderer* fr = app->fontRenderer;

    // Push screen dimensions for text pipeline
    float screenDimensions[2] = {
        (float)app->swapChainExtent.width,
        (float)app->swapChainExtent.height
    };

    // Draw text
    vkCmdBindPipeline(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS,
                     fr->textPipeline);

    vkCmdPushConstants(commandBuffer, fr->textPipelineLayout,
                      VK_SHADER_STAGE_VERTEX_BIT, 0, sizeof(screenDimensions),
                      screenDimensions);

    VkBuffer textBuffers[] = {fr->textVertexBuffer};
    VkDeviceSize offsets[] = {0};
    vkCmdBindVertexBuffers(commandBuffer, 0, 1, textBuffers, offsets);

    vkCmdBindDescriptorSets(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS,
                           fr->textPipelineLayout, 0, 1,
                           &fr->textDescriptorSets[app->currentFrame],
                           0, NULL);

    if (fr->textVertexCount > 0) {
        vkCmdDraw(commandBuffer, fr->textVertexCount, 1, 0, 0);
    }
}
