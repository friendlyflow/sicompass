#include "rectangle.h"
#include "main.h"

// Forward declarations of helper functions from main.c
extern void createBuffer(SiCompassApplication* app, VkDeviceSize size, VkBufferUsageFlags usage,
                        VkMemoryPropertyFlags properties, VkBuffer* buffer, VkDeviceMemory* bufferMemory);
extern VkShaderModule createShaderModule(VkDevice device, const char* code, size_t codeSize);
extern char* readFile(const char* filename, size_t* outSize);

// ============================================================================
// RECTANGLE RENDERING FUNCTIONS
// ============================================================================

void createRectangleVertexBuffer(SiCompassApplication* app) {
    RectangleRenderer* rr = app->rectangleRenderer;
    VkDeviceSize bufferSize = sizeof(RectangleVertex) * 6 * MAX_RECTANGLES;

    createBuffer(app, bufferSize, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                 VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                 &rr->vertexBuffer, &rr->vertexBufferMemory);

    // Persistently map the vertex buffer for efficient updates
    vkMapMemory(app->device, rr->vertexBufferMemory, 0, bufferSize, 0,
                (void**)&rr->mappedVertexData);

    rr->vertexCount = 0;
}

void createRectanglePipeline(SiCompassApplication* app) {
    RectangleRenderer* rr = app->rectangleRenderer;

    size_t vertSize, fragSize;
    char* vertCode = readFile("shaders/rectangle_vert.spv", &vertSize);
    char* fragCode = readFile("shaders/rectangle_frag.spv", &fragSize);

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
    binding.stride = sizeof(RectangleVertex);
    binding.inputRate = VK_VERTEX_INPUT_RATE_VERTEX;

    VkVertexInputAttributeDescription attrs[5] = {0};
    attrs[0].location = 0;
    attrs[0].binding = 0;
    attrs[0].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[0].offset = offsetof(RectangleVertex, pos);

    attrs[1].location = 1;
    attrs[1].binding = 0;
    attrs[1].format = VK_FORMAT_R32G32B32A32_SFLOAT;
    attrs[1].offset = offsetof(RectangleVertex, color);

    attrs[2].location = 2;
    attrs[2].binding = 0;
    attrs[2].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[2].offset = offsetof(RectangleVertex, cornerRadius);

    attrs[3].location = 3;
    attrs[3].binding = 0;
    attrs[3].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[3].offset = offsetof(RectangleVertex, rectSize);

    attrs[4].location = 4;
    attrs[4].binding = 0;
    attrs[4].format = VK_FORMAT_R32G32_SFLOAT;
    attrs[4].offset = offsetof(RectangleVertex, rectOrigin);

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

    vkCreatePipelineLayout(app->device, &layoutInfo, NULL, &rr->pipelineLayout);

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
    pipelineInfo.layout = rr->pipelineLayout;
    pipelineInfo.renderPass = app->renderPass;
    pipelineInfo.subpass = 0;

    vkCreateGraphicsPipelines(app->device, VK_NULL_HANDLE, 1, &pipelineInfo, NULL, &rr->pipeline);

    vkDestroyShaderModule(app->device, fragModule, NULL);
    vkDestroyShaderModule(app->device, vertModule, NULL);
    free(vertCode);
    free(fragCode);
}

void beginRectangleRendering(SiCompassApplication* app) {
    RectangleRenderer* rr = app->rectangleRenderer;
    rr->vertexCount = 0;  // Reset rectangle count for new frame
}

void prepareRectangle(SiCompassApplication* app,
                     float x, float y, float width, float height,
                     vec4 color, float cornerRadius) {
    RectangleRenderer* rr = app->rectangleRenderer;

    // Check if we've hit the limit
    if (rr->vertexCount >= 6 * MAX_RECTANGLES) {
        fprintf(stderr, "Warning: Maximum rectangle count (%d) exceeded\n", MAX_RECTANGLES);
        return;
    }

    float minX = x;
    float minY = y;
    float maxX = x + width;
    float maxY = y + height;

    // Clamp corner radius to prevent it from being larger than the rectangle
    float maxRadius = fminf(width, height) * 0.5f;
    float actualCornerRadius = fminf(cornerRadius, maxRadius);

    // Write directly to mapped buffer at current offset
    RectangleVertex* vertices = &rr->mappedVertexData[rr->vertexCount];

    // Bottom-left corner is our reference point
    for (int i = 0; i < 6; i++) {
        vertices[i].color[0] = color[0];
        vertices[i].color[1] = color[1];
        vertices[i].color[2] = color[2];
        vertices[i].color[3] = color[3];
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

    rr->vertexCount += 6;  // Increment by 6 vertices (one rectangle)
}

void drawRectangle(SiCompassApplication* app, VkCommandBuffer commandBuffer) {
    RectangleRenderer* rr = app->rectangleRenderer;

    // Push screen dimensions for rectangle pipeline
    float screenDimensions[2] = {
        (float)app->swapChainExtent.width,
        (float)app->swapChainExtent.height
    };

    // Draw rectangle
    if (rr->vertexCount > 0) {
        vkCmdBindPipeline(commandBuffer, VK_PIPELINE_BIND_POINT_GRAPHICS,
                         rr->pipeline);

        vkCmdPushConstants(commandBuffer, rr->pipelineLayout,
                          VK_SHADER_STAGE_VERTEX_BIT, 0, sizeof(screenDimensions),
                          screenDimensions);

        VkBuffer rectangleBuffers[] = {rr->vertexBuffer};
        VkDeviceSize offsets[] = {0};
        vkCmdBindVertexBuffers(commandBuffer, 0, 1, rectangleBuffers, offsets);

        vkCmdDraw(commandBuffer, rr->vertexCount, 1, 0, 0);
    }
}

void cleanupRectangleRenderer(SiCompassApplication* app) {
    RectangleRenderer* rr = app->rectangleRenderer;

    vkDestroyPipeline(app->device, rr->pipeline, NULL);
    vkDestroyPipelineLayout(app->device, rr->pipelineLayout, NULL);
    vkDestroyBuffer(app->device, rr->vertexBuffer, NULL);
    vkFreeMemory(app->device, rr->vertexBufferMemory, NULL);

    free(app->rectangleRenderer);
}
