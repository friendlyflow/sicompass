#pragma once

#include <vulkan/vulkan.h>

// Forward declaration
typedef struct SiCompassApplication SiCompassApplication;

// Texture image creation and management
void createTextureImage(SiCompassApplication* app);
void createTextureImageView(SiCompassApplication* app);
void createTextureSampler(SiCompassApplication* app);
void cleanupTextureResources(SiCompassApplication* app);
