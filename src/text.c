#include "text.h"
#include "main.h"

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
    fr->dpi = (float)scaledDPI;

    // Use larger base size (64pt) for better quality when scaling down
    FT_Set_Char_Size(fr->ftFace, 0, 64*64, scaledDPI, scaledDPI);

    // Store font metrics for consistent line height
    fr->ascender = fr->ftFace->size->metrics.ascender / 64.0f;
    fr->descender = fr->ftFace->size->metrics.descender / 64.0f;
    fr->lineHeight = fr->ascender - fr->descender;

    // Initialize HarfBuzz
    fr->hbFont = hb_ft_font_create(fr->ftFace, NULL);
    if (!fr->hbFont) {
        fprintf(stderr, "Failed to create HarfBuzz font\n");
        exit(EXIT_FAILURE);
    }

    // Create HarfBuzz buffer for text shaping
    fr->hbBuffer = hb_buffer_create();
    if (!fr->hbBuffer) {
        fprintf(stderr, "Failed to create HarfBuzz buffer\n");
        exit(EXIT_FAILURE);
    }

}

void createFontAtlas(SiCompassApplication* app) {
    FontRenderer* fr = app->fontRenderer;

    unsigned char* atlasData = (unsigned char*)calloc(FONT_ATLAS_SIZE * FONT_ATLAS_SIZE, 1);

    int penX = 0, penY = 0, rowHeight = 0;

    // Include ASCII (32-127) and Latin-1 Supplement (128-255) for accented characters
    for (unsigned int c = 32; c < 256; c++) {
        // Use FT_LOAD_TARGET_LIGHT for better antialiasing quality
        if (FT_Load_Char(fr->ftFace, c, FT_LOAD_RENDER | FT_LOAD_TARGET_LIGHT)) continue;

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

void calculateTextBounds(SiCompassApplication* app, const char* text,
                        float x, float y, float scale,
                        float* outMinX, float* outMinY,
                        float* outMaxX, float* outMaxY) {
    FontRenderer* fr = app->fontRenderer;

    // Use HarfBuzz for proper text shaping (same as rendering)
    hb_buffer_clear_contents(fr->hbBuffer);
    hb_buffer_set_direction(fr->hbBuffer, HB_DIRECTION_LTR);
    hb_buffer_set_script(fr->hbBuffer, HB_SCRIPT_LATIN);
    hb_buffer_set_language(fr->hbBuffer, hb_language_from_string("en", -1));
    hb_buffer_add_utf8(fr->hbBuffer, text, -1, 0, -1);
    hb_shape(fr->hbFont, fr->hbBuffer, NULL, 0);

    unsigned int glyphCount;
    hb_glyph_info_t* glyphInfo = hb_buffer_get_glyph_infos(fr->hbBuffer, &glyphCount);
    hb_glyph_position_t* glyphPos = hb_buffer_get_glyph_positions(fr->hbBuffer, &glyphCount);

    float cursorX = x;
    float minX = x;
    float maxX = x;
    bool first = true;

    // Use consistent line height based on font metrics
    float minY = y - fr->ascender * scale;
    float maxY = y - fr->descender * scale;

    // Calculate bounds using HarfBuzz advance values
    // This correctly handles all glyphs including those with indices >= 256
    for (unsigned int i = 0; i < glyphCount; i++) {
        cursorX += (glyphPos[i].x_advance / 64.0f) * scale;
    }

    // Use simple bounds from start to end
    if (glyphCount > 0) {
        minX = x;
        maxX = cursorX;
    }

    *outMinX = minX;
    *outMinY = minY;
    *outMaxX = maxX;
    *outMaxY = maxY;
}

float getTextScale(SiCompassApplication* app, float desiredSizePt) {
    FontRenderer* fr = app->fontRenderer;

    // Convert points to pixels: pixels = points * DPI / 72
    float desiredHeightPx = desiredSizePt * fr->dpi / 72.0f;

    // Calculate scale factor to achieve desired pixel height
    // desiredHeightPx = lineHeight * scale
    // scale = desiredHeightPx / lineHeight
    return desiredHeightPx / fr->lineHeight;
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

void beginTextRendering(SiCompassApplication* app) {
    app->fontRenderer->textVertexCount = 0;
}

void prepareTextForRendering(SiCompassApplication* app, const char* text,
                             float x, float y, float scale, uint32_t color) {
    FontRenderer* fr = app->fontRenderer;

    // Convert uint32_t color to vec3
    vec3 colorVec;
    colorVec[0] = ((color >> 24) & 0xFF) / 255.0f;
    colorVec[1] = ((color >> 16) & 0xFF) / 255.0f;
    colorVec[2] = ((color >> 8) & 0xFF) / 255.0f;

    TextVertex vertices[MAX_TEXT_VERTICES];
    uint32_t vi = 0;

    // Clear HarfBuzz buffer and add text
    hb_buffer_clear_contents(fr->hbBuffer);
    hb_buffer_set_direction(fr->hbBuffer, HB_DIRECTION_LTR);
    hb_buffer_set_script(fr->hbBuffer, HB_SCRIPT_LATIN);
    hb_buffer_set_language(fr->hbBuffer, hb_language_from_string("en", -1));

    // Add UTF-8 text to buffer
    hb_buffer_add_utf8(fr->hbBuffer, text, -1, 0, -1);

    // Shape the text
    hb_shape(fr->hbFont, fr->hbBuffer, NULL, 0);

    // Get shaped glyph information
    unsigned int glyphCount;
    hb_glyph_info_t* glyphInfo = hb_buffer_get_glyph_infos(fr->hbBuffer, &glyphCount);
    hb_glyph_position_t* glyphPos = hb_buffer_get_glyph_positions(fr->hbBuffer, &glyphCount);

    float cursorX = x;
    float cursorY = y;

    for (unsigned int i = 0; i < glyphCount; i++) {
        hb_codepoint_t glyphIndex = glyphInfo[i].codepoint;

        // Get the Unicode codepoint from the glyph info
        // HarfBuzz provides us with the shaped glyph, we need to find the original character
        unsigned int cluster = glyphInfo[i].cluster;

        // Decode UTF-8 character at cluster position
        unsigned int charCode = 0;
        const unsigned char* utf8 = (const unsigned char*)&text[cluster];

        if (utf8[0] < 0x80) {
            // Single-byte ASCII character (0xxxxxxx)
            charCode = utf8[0];
        } else if ((utf8[0] & 0xE0) == 0xC0) {
            // Two-byte character (110xxxxx 10xxxxxx)
            charCode = ((utf8[0] & 0x1F) << 6) | (utf8[1] & 0x3F);
        } else if ((utf8[0] & 0xF0) == 0xE0) {
            // Three-byte character (1110xxxx 10xxxxxx 10xxxxxx)
            charCode = ((utf8[0] & 0x0F) << 12) | ((utf8[1] & 0x3F) << 6) | (utf8[2] & 0x3F);
        } else if ((utf8[0] & 0xF8) == 0xF0) {
            // Four-byte character (11110xxx 10xxxxxx 10xxxxxx 10xxxxxx)
            charCode = ((utf8[0] & 0x07) << 18) | ((utf8[1] & 0x3F) << 12) |
                      ((utf8[2] & 0x3F) << 6) | (utf8[3] & 0x3F);
        }

        // Check if character is in our atlas (32-255)
        GlyphInfo* g = NULL;
        if (charCode >= 32 && charCode < 256) {
            g = &fr->glyphs[charCode];
        } else {
            // Skip characters outside our atlas range
            cursorX += (glyphPos[i].x_advance / 64.0f) * scale;
            cursorY += (glyphPos[i].y_advance / 64.0f) * scale;
            continue;
        }

        // Apply HarfBuzz positioning
        float xOffset = (glyphPos[i].x_offset / 64.0f) * scale;
        float yOffset = (glyphPos[i].y_offset / 64.0f) * scale;

        float xpos = cursorX + xOffset + g->bearing[0] * scale;
        float ypos = cursorY + yOffset - g->bearing[1] * scale;
        float w = g->size[0] * scale;
        float h = g->size[1] * scale;

        if (vi + 6 > MAX_TEXT_VERTICES) break;

        vertices[vi++] = (TextVertex){{xpos, ypos + h, 0.0f}, {g->uvMin[0], g->uvMax[1]}, {colorVec[0], colorVec[1], colorVec[2]}};
        vertices[vi++] = (TextVertex){{xpos, ypos, 0.0f}, {g->uvMin[0], g->uvMin[1]}, {colorVec[0], colorVec[1], colorVec[2]}};
        vertices[vi++] = (TextVertex){{xpos + w, ypos, 0.0f}, {g->uvMax[0], g->uvMin[1]}, {colorVec[0], colorVec[1], colorVec[2]}};

        vertices[vi++] = (TextVertex){{xpos, ypos + h, 0.0f}, {g->uvMin[0], g->uvMax[1]}, {colorVec[0], colorVec[1], colorVec[2]}};
        vertices[vi++] = (TextVertex){{xpos + w, ypos, 0.0f}, {g->uvMax[0], g->uvMin[1]}, {colorVec[0], colorVec[1], colorVec[2]}};
        vertices[vi++] = (TextVertex){{xpos + w, ypos + h, 0.0f}, {g->uvMax[0], g->uvMax[1]}, {colorVec[0], colorVec[1], colorVec[2]}};

        cursorX += (glyphPos[i].x_advance / 64.0f) * scale;
        cursorY += (glyphPos[i].y_advance / 64.0f) * scale;
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

    // Cleanup HarfBuzz resources
    if (fr->hbBuffer) {
        hb_buffer_destroy(fr->hbBuffer);
    }
    if (fr->hbFont) {
        hb_font_destroy(fr->hbFont);
    }

    FT_Done_Face(fr->ftFace);
    FT_Done_FreeType(fr->ftLibrary);

    free(app->fontRenderer);
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
