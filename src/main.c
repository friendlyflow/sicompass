#include "main.h"
#include "view.h"

#define STB_IMAGE_IMPLEMENTATION
#include <stb_image.h>

const uint32_t WIDTH = 800;
const uint32_t HEIGHT = 600;

const char* validationLayers[] = {
    "VK_LAYER_KHRONOS_validation"
};
const uint32_t validationLayerCount = 1;

const char* deviceExtensions[] = {
    VK_KHR_SWAPCHAIN_EXTENSION_NAME
};
const uint32_t deviceExtensionCount = 1;

#ifdef NDEBUG
const bool enableValidationLayers = false;
#else
const bool enableValidationLayers = true;
#endif

VkResult CreateDebugUtilsMessengerEXT(VkInstance instance, const VkDebugUtilsMessengerCreateInfoEXT* pCreateInfo, 
                                      const VkAllocationCallbacks* pAllocator, VkDebugUtilsMessengerEXT* pDebugMessenger) {
    PFN_vkCreateDebugUtilsMessengerEXT func = (PFN_vkCreateDebugUtilsMessengerEXT)vkGetInstanceProcAddr(instance, "vkCreateDebugUtilsMessengerEXT");
    if (func != NULL) {
        return func(instance, pCreateInfo, pAllocator, pDebugMessenger);
    }
    return VK_ERROR_EXTENSION_NOT_PRESENT;
}

void DestroyDebugUtilsMessengerEXT(VkInstance instance, VkDebugUtilsMessengerEXT debugMessenger, const VkAllocationCallbacks* pAllocator) {
    PFN_vkDestroyDebugUtilsMessengerEXT func = (PFN_vkDestroyDebugUtilsMessengerEXT)vkGetInstanceProcAddr(instance, "vkDestroyDebugUtilsMessengerEXT");
    if (func != NULL) {
        func(instance, debugMessenger, pAllocator);
    }
}

bool isQueueFamilyComplete(QueueFamilyIndices* indices) {
    return indices->hasGraphicsFamily && indices->hasPresentFamily;
}

typedef struct {
    VkSurfaceCapabilitiesKHR capabilities;
    VkSurfaceFormatKHR* formats;
    uint32_t formatCount;
    VkPresentModeKHR* presentModes;
    uint32_t presentModeCount;
} SwapChainSupportDetails;

void freeSwapChainSupportDetails(SwapChainSupportDetails* details) {
    if (details->formats) free(details->formats);
    if (details->presentModes) free(details->presentModes);
}

// Initialize the application struct
void siCompassApplicationInit(SiCompassApplication* app) {
    app->window = NULL;
    app->instance = VK_NULL_HANDLE;
    app->debugMessenger = VK_NULL_HANDLE;
    app->surface = VK_NULL_HANDLE;
    app->physicalDevice = VK_NULL_HANDLE;
    // app->msaaSamples = VK_SAMPLE_COUNT_1_BIT;
    app->device = VK_NULL_HANDLE;
    app->graphicsQueue = VK_NULL_HANDLE;
    app->presentQueue = VK_NULL_HANDLE;
    app->swapChain = VK_NULL_HANDLE;
    
    app->swapChainImages = NULL;
    app->swapChainImageCount = 0;
    app->swapChainImageViews = NULL;
    app->swapChainImageViewCount = 0;
    app->swapChainFramebuffers = NULL;
    app->swapChainFramebufferCount = 0;
    
    app->renderPass = VK_NULL_HANDLE;
    app->descriptorSetLayout = VK_NULL_HANDLE;
    app->pipelineLayout = VK_NULL_HANDLE;
    app->graphicsPipeline = VK_NULL_HANDLE;
    app->commandPool = VK_NULL_HANDLE;

    // app->colorImage = VK_NULL_HANDLE;
    // app->colorImageMemory = VK_NULL_HANDLE;
    // app->colorImageView = VK_NULL_HANDLE;
    
    app->depthImage = VK_NULL_HANDLE;
    app->depthImageMemory = VK_NULL_HANDLE;
    app->depthImageView = VK_NULL_HANDLE;
    
    // app->mipLevels = 0;
    app->textureImage = VK_NULL_HANDLE;
    app->textureImageMemory = VK_NULL_HANDLE;
    app->textureImageView = VK_NULL_HANDLE;
    app->textureSampler = VK_NULL_HANDLE;

    // app->vertices = NULL;
    // app->vertexCount = 0;
    // app->indices = NULL;
    // app->indexCount = 0;
    
    app->vertexBuffer = VK_NULL_HANDLE;
    app->vertexBufferMemory = VK_NULL_HANDLE;
    app->indexBuffer = VK_NULL_HANDLE;
    app->indexBufferMemory = VK_NULL_HANDLE;
    
    memset(app->uniformBuffers, 0, sizeof(app->uniformBuffers));
    memset(app->uniformBuffersMemory, 0, sizeof(app->uniformBuffersMemory));
    memset(app->uniformBuffersMapped, 0, sizeof(app->uniformBuffersMapped));
    app->uniformBufferCount = 0;
    
    app->descriptorPool = VK_NULL_HANDLE;
    memset(app->descriptorSets, 0, sizeof(app->descriptorSets));
    app->descriptorSetCount = 0;
    
    memset(app->commandBuffers, 0, sizeof(app->commandBuffers));
    app->commandBufferCount = 0;
    
    memset(app->imageAvailableSemaphores, 0, sizeof(app->imageAvailableSemaphores));
    memset(app->renderFinishedSemaphores, 0, sizeof(app->renderFinishedSemaphores));
    memset(app->inFlightFences, 0, sizeof(app->inFlightFences));
    // app->syncObjectCount = 0;
    
    app->currentFrame = 0;
    app->framebufferResized = false;
    app->running = true;

    app->startTime = clock();

    app->fontRenderer = (FontRenderer*)calloc(1, sizeof(FontRenderer));
}

// Forward declarations
char** getRequiredExtensions(uint32_t* extensionCount);
char* readFile(const char* filename, size_t* fileSize);
VKAPI_ATTR VkBool32 VKAPI_CALL debugCallback(VkDebugUtilsMessageSeverityFlagBitsEXT messageSeverity,
                                              VkDebugUtilsMessageTypeFlagsEXT messageType,
                                              const VkDebugUtilsMessengerCallbackDataEXT* pCallbackData,
                                              void* pUserData);

void initWindow(SiCompassApplication* app) {
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        fprintf(stderr, "Failed to initialize SDL: %s\n", SDL_GetError());
        exit(EXIT_FAILURE);
    }

    app->window = SDL_CreateWindow("sicompass", WIDTH, HEIGHT, SDL_WINDOW_VULKAN | SDL_WINDOW_RESIZABLE);
    if (!app->window) {
        fprintf(stderr, "Failed to create window: %s\n", SDL_GetError());
        SDL_Quit();
        exit(EXIT_FAILURE);
    }
}

void populateDebugMessengerCreateInfo(VkDebugUtilsMessengerCreateInfoEXT* createInfo) {
    memset(createInfo, 0, sizeof(VkDebugUtilsMessengerCreateInfoEXT));
    createInfo->sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT;
    createInfo->messageSeverity = VK_DEBUG_UTILS_MESSAGE_SEVERITY_VERBOSE_BIT_EXT | 
                                  VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT | 
                                  VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT;
    createInfo->messageType = VK_DEBUG_UTILS_MESSAGE_TYPE_GENERAL_BIT_EXT | 
                             VK_DEBUG_UTILS_MESSAGE_TYPE_VALIDATION_BIT_EXT | 
                             VK_DEBUG_UTILS_MESSAGE_TYPE_PERFORMANCE_BIT_EXT;
    createInfo->pfnUserCallback = debugCallback;
}

bool checkValidationLayerSupport(void) {
    uint32_t layerCount;
    vkEnumerateInstanceLayerProperties(&layerCount, NULL);

    VkLayerProperties* availableLayers = malloc(sizeof(VkLayerProperties) * layerCount);
    vkEnumerateInstanceLayerProperties(&layerCount, availableLayers);

    for (uint32_t i = 0; i < validationLayerCount; i++) {
        bool layerFound = false;

        for (uint32_t j = 0; j < layerCount; j++) {
            if (strcmp(validationLayers[i], availableLayers[j].layerName) == 0) {
                layerFound = true;
                break;
            }
        }

        if (!layerFound) {
            free(availableLayers);
            return false;
        }
    }

    free(availableLayers);
    return true;
}

void createInstance(SiCompassApplication* app) {
    if (enableValidationLayers && !checkValidationLayerSupport()) {
        fprintf(stderr, "Validation layers requested, but not available!\n");
        exit(EXIT_FAILURE);
    }

    VkApplicationInfo appInfo = {0};
    appInfo.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    appInfo.pApplicationName = "sicompass";
    appInfo.applicationVersion = VK_MAKE_VERSION(1, 0, 0);
    appInfo.pEngineName = "No Engine";
    appInfo.engineVersion = VK_MAKE_VERSION(1, 0, 0);
    appInfo.apiVersion = VK_API_VERSION_1_0;

    VkInstanceCreateInfo createInfo = {0};
    createInfo.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    createInfo.pApplicationInfo = &appInfo;

    uint32_t extensionCount = 0;
    char** extensions = getRequiredExtensions(&extensionCount);
    createInfo.enabledExtensionCount = extensionCount;
    createInfo.ppEnabledExtensionNames = (const char* const*)extensions;

    VkDebugUtilsMessengerCreateInfoEXT debugCreateInfo = {0};
    if (enableValidationLayers) {
        createInfo.enabledLayerCount = validationLayerCount;
        createInfo.ppEnabledLayerNames = validationLayers;
        populateDebugMessengerCreateInfo(&debugCreateInfo);
        createInfo.pNext = &debugCreateInfo;
    } else {
        createInfo.enabledLayerCount = 0;
        createInfo.pNext = NULL;
    }

    if (vkCreateInstance(&createInfo, NULL, &app->instance) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create instance!\n");
        exit(EXIT_FAILURE);
    }

    free(extensions);
}

void setupDebugMessenger(SiCompassApplication* app) {
    if (!enableValidationLayers) return;

    VkDebugUtilsMessengerCreateInfoEXT createInfo;
    populateDebugMessengerCreateInfo(&createInfo);

    if (CreateDebugUtilsMessengerEXT(app->instance, &createInfo, NULL, &app->debugMessenger) != VK_SUCCESS) {
        fprintf(stderr, "Failed to set up debug messenger!\n");
        exit(EXIT_FAILURE);
    }
}

void createSurface(SiCompassApplication* app) {
    if (!SDL_Vulkan_CreateSurface(app->window, app->instance, NULL, &app->surface)) {
        fprintf(stderr, "Failed to create window surface: %s\n", SDL_GetError());
        exit(EXIT_FAILURE);
    }
}

char** getRequiredExtensions(uint32_t* extensionCount) {
    // Get SDL required extensions
    uint32_t sdlExtensionCount = 0;
    const char* const* sdlExtensions = SDL_Vulkan_GetInstanceExtensions(&sdlExtensionCount);
    
    if (!sdlExtensions) {
        fprintf(stderr, "Failed to get SDL Vulkan extensions: %s\n", SDL_GetError());
        exit(EXIT_FAILURE);
    }

    uint32_t totalExtensionCount = sdlExtensionCount;
    if (enableValidationLayers) {
        totalExtensionCount++;
    }

    char** extensions = malloc(sizeof(char*) * totalExtensionCount);
    for (uint32_t i = 0; i < sdlExtensionCount; i++) {
        extensions[i] = (char*)sdlExtensions[i];
    }

    if (enableValidationLayers) {
        extensions[sdlExtensionCount] = (char*)VK_EXT_DEBUG_UTILS_EXTENSION_NAME;
    }

    *extensionCount = totalExtensionCount;
    return extensions;
}

char* readFile(const char* filename, size_t* fileSize) {
    FILE* file = fopen(filename, "rb");

    if (!file) {
        fprintf(stderr, "Failed to open file: %s\n", filename);
        exit(EXIT_FAILURE);
    }

    fseek(file, 0, SEEK_END);
    *fileSize = ftell(file);
    fseek(file, 0, SEEK_SET);

    char* buffer = malloc(*fileSize);
    fread(buffer, 1, *fileSize, file);

    fclose(file);

    return buffer;
}

VKAPI_ATTR VkBool32 VKAPI_CALL debugCallback(VkDebugUtilsMessageSeverityFlagBitsEXT messageSeverity,
                                              VkDebugUtilsMessageTypeFlagsEXT messageType,
                                              const VkDebugUtilsMessengerCallbackDataEXT* pCallbackData,
                                              void* pUserData) {
    fprintf(stderr, "validation layer: %s\n", pCallbackData->pMessage);
    return VK_FALSE;
}

QueueFamilyIndices findQueueFamilies(VkPhysicalDevice device, VkSurfaceKHR surface) {
    QueueFamilyIndices indices = {0};
    indices.hasGraphicsFamily = false;
    indices.hasPresentFamily = false;

    uint32_t queueFamilyCount = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(device, &queueFamilyCount, NULL);

    VkQueueFamilyProperties* queueFamilies = malloc(sizeof(VkQueueFamilyProperties) * queueFamilyCount);
    vkGetPhysicalDeviceQueueFamilyProperties(device, &queueFamilyCount, queueFamilies);

    for (uint32_t i = 0; i < queueFamilyCount; i++) {
        if (queueFamilies[i].queueFlags & VK_QUEUE_GRAPHICS_BIT) {
            indices.graphicsFamily = i;
            indices.hasGraphicsFamily = true;
        }

        VkBool32 presentSupport = false;
        vkGetPhysicalDeviceSurfaceSupportKHR(device, i, surface, &presentSupport);

        if (presentSupport) {
            indices.presentFamily = i;
            indices.hasPresentFamily = true;
        }

        if (isQueueFamilyComplete(&indices)) {
            break;
        }
    }

    free(queueFamilies);
    return indices;
}

bool checkDeviceExtensionSupport(VkPhysicalDevice device) {
    uint32_t extensionCount;
    vkEnumerateDeviceExtensionProperties(device, NULL, &extensionCount, NULL);

    VkExtensionProperties* availableExtensions = malloc(sizeof(VkExtensionProperties) * extensionCount);
    vkEnumerateDeviceExtensionProperties(device, NULL, &extensionCount, availableExtensions);

    uint32_t foundCount = 0;
    for (uint32_t i = 0; i < deviceExtensionCount; i++) {
        for (uint32_t j = 0; j < extensionCount; j++) {
            if (strcmp(deviceExtensions[i], availableExtensions[j].extensionName) == 0) {
                foundCount++;
                break;
            }
        }
    }

    free(availableExtensions);
    return foundCount == deviceExtensionCount;
}

SwapChainSupportDetails querySwapChainSupport(VkPhysicalDevice device, VkSurfaceKHR surface) {
    SwapChainSupportDetails details = {0};

    vkGetPhysicalDeviceSurfaceCapabilitiesKHR(device, surface, &details.capabilities);

    vkGetPhysicalDeviceSurfaceFormatsKHR(device, surface, &details.formatCount, NULL);
    if (details.formatCount != 0) {
        details.formats = malloc(sizeof(VkSurfaceFormatKHR) * details.formatCount);
        vkGetPhysicalDeviceSurfaceFormatsKHR(device, surface, &details.formatCount, details.formats);
    }

    vkGetPhysicalDeviceSurfacePresentModesKHR(device, surface, &details.presentModeCount, NULL);
    if (details.presentModeCount != 0) {
        details.presentModes = malloc(sizeof(VkPresentModeKHR) * details.presentModeCount);
        vkGetPhysicalDeviceSurfacePresentModesKHR(device, surface, &details.presentModeCount, details.presentModes);
    }

    return details;
}

bool isDeviceSuitable(VkPhysicalDevice device, VkSurfaceKHR surface) {
    QueueFamilyIndices indices = findQueueFamilies(device, surface);

    bool extensionsSupported = checkDeviceExtensionSupport(device);

    bool swapChainAdequate = false;
    if (extensionsSupported) {
        SwapChainSupportDetails swapChainSupport = querySwapChainSupport(device, surface);
        swapChainAdequate = swapChainSupport.formatCount > 0 && swapChainSupport.presentModeCount > 0;
        freeSwapChainSupportDetails(&swapChainSupport);
    }

    VkPhysicalDeviceFeatures supportedFeatures;
    vkGetPhysicalDeviceFeatures(device, &supportedFeatures);

    return isQueueFamilyComplete(&indices) && extensionsSupported && swapChainAdequate && supportedFeatures.samplerAnisotropy;
}

void pickPhysicalDevice(SiCompassApplication* app) {
    uint32_t deviceCount = 0;
    vkEnumeratePhysicalDevices(app->instance, &deviceCount, NULL);

    if (deviceCount == 0) {
        fprintf(stderr, "Failed to find GPUs with Vulkan support!\n");
        exit(EXIT_FAILURE);
    }

    VkPhysicalDevice* devices = malloc(sizeof(VkPhysicalDevice) * deviceCount);
    vkEnumeratePhysicalDevices(app->instance, &deviceCount, devices);

    app->physicalDevice = VK_NULL_HANDLE;
    for (uint32_t i = 0; i < deviceCount; i++) {
        if (isDeviceSuitable(devices[i], app->surface)) {
            app->physicalDevice = devices[i];
            break;
        }
    }

    free(devices);

    if (app->physicalDevice == VK_NULL_HANDLE) {
        fprintf(stderr, "Failed to find a suitable GPU!\n");
        exit(EXIT_FAILURE);
    }
}

void createLogicalDevice(SiCompassApplication* app) {
    QueueFamilyIndices indices = findQueueFamilies(app->physicalDevice, app->surface);

    VkDeviceQueueCreateInfo queueCreateInfos[2] = {0};
    uint32_t uniqueQueueFamilies[2];
    uint32_t uniqueQueueFamilyCount = 0;

    if (indices.graphicsFamily == indices.presentFamily) {
        uniqueQueueFamilies[0] = indices.graphicsFamily;
        uniqueQueueFamilyCount = 1;
    } else {
        uniqueQueueFamilies[0] = indices.graphicsFamily;
        uniqueQueueFamilies[1] = indices.presentFamily;
        uniqueQueueFamilyCount = 2;
    }

    float queuePriority = 1.0f;
    for (uint32_t i = 0; i < uniqueQueueFamilyCount; i++) {
        queueCreateInfos[i].sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
        queueCreateInfos[i].queueFamilyIndex = uniqueQueueFamilies[i];
        queueCreateInfos[i].queueCount = 1;
        queueCreateInfos[i].pQueuePriorities = &queuePriority;
    }

    VkPhysicalDeviceFeatures deviceFeatures = {0};
    deviceFeatures.samplerAnisotropy = VK_TRUE;

    VkDeviceCreateInfo createInfo = {0};
    createInfo.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO;
    createInfo.queueCreateInfoCount = uniqueQueueFamilyCount;
    createInfo.pQueueCreateInfos = queueCreateInfos;
    createInfo.pEnabledFeatures = &deviceFeatures;
    createInfo.enabledExtensionCount = deviceExtensionCount;
    createInfo.ppEnabledExtensionNames = deviceExtensions;

    if (enableValidationLayers) {
        createInfo.enabledLayerCount = validationLayerCount;
        createInfo.ppEnabledLayerNames = validationLayers;
    } else {
        createInfo.enabledLayerCount = 0;
    }

    if (vkCreateDevice(app->physicalDevice, &createInfo, NULL, &app->device) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create logical device!\n");
        exit(EXIT_FAILURE);
    }

    vkGetDeviceQueue(app->device, indices.graphicsFamily, 0, &app->graphicsQueue);
    vkGetDeviceQueue(app->device, indices.presentFamily, 0, &app->presentQueue);
}

VkSurfaceFormatKHR chooseSwapSurfaceFormat(const VkSurfaceFormatKHR* availableFormats, uint32_t formatCount) {
    for (uint32_t i = 0; i < formatCount; i++) {
        if (availableFormats[i].format == VK_FORMAT_B8G8R8A8_SRGB && 
            availableFormats[i].colorSpace == VK_COLOR_SPACE_SRGB_NONLINEAR_KHR) {
            return availableFormats[i];
        }
    }
    return availableFormats[0];
}

VkPresentModeKHR chooseSwapPresentMode(const VkPresentModeKHR* availablePresentModes, uint32_t modeCount) {
    for (uint32_t i = 0; i < modeCount; i++) {
        if (availablePresentModes[i] == VK_PRESENT_MODE_MAILBOX_KHR) {
            return availablePresentModes[i];
        }
    }
    return VK_PRESENT_MODE_FIFO_KHR;
}

uint32_t clamp_uint32(uint32_t value, uint32_t min, uint32_t max) {
    if (value < min) return min;
    if (value > max) return max;
    return value;
}

VkExtent2D chooseSwapExtent(const VkSurfaceCapabilitiesKHR* capabilities, SDL_Window* window) {
    if (capabilities->currentExtent.width != UINT32_MAX) {
        return capabilities->currentExtent;
    } else {
        int width, height;
        SDL_GetWindowSizeInPixels(window, &width, &height);

        VkExtent2D actualExtent = {(uint32_t)width, (uint32_t)height};

        actualExtent.width = clamp_uint32(actualExtent.width, 
                                         capabilities->minImageExtent.width, 
                                         capabilities->maxImageExtent.width);
        actualExtent.height = clamp_uint32(actualExtent.height, 
                                          capabilities->minImageExtent.height, 
                                          capabilities->maxImageExtent.height);

        return actualExtent;
    }
}

void createSwapChain(SiCompassApplication* app) {
    SwapChainSupportDetails swapChainSupport = querySwapChainSupport(app->physicalDevice, app->surface);

    VkSurfaceFormatKHR surfaceFormat = chooseSwapSurfaceFormat(swapChainSupport.formats, swapChainSupport.formatCount);
    VkPresentModeKHR presentMode = chooseSwapPresentMode(swapChainSupport.presentModes, swapChainSupport.presentModeCount);
    VkExtent2D extent = chooseSwapExtent(&swapChainSupport.capabilities, app->window);

    uint32_t imageCount = swapChainSupport.capabilities.minImageCount + 1;
    if (swapChainSupport.capabilities.maxImageCount > 0 && imageCount > swapChainSupport.capabilities.maxImageCount) {
        imageCount = swapChainSupport.capabilities.maxImageCount;
    }

    VkSwapchainCreateInfoKHR createInfo = {0};
    createInfo.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR;
    createInfo.surface = app->surface;
    createInfo.minImageCount = imageCount;
    createInfo.imageFormat = surfaceFormat.format;
    createInfo.imageColorSpace = surfaceFormat.colorSpace;
    createInfo.imageExtent = extent;
    createInfo.imageArrayLayers = 1;
    createInfo.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT;

    QueueFamilyIndices indices = findQueueFamilies(app->physicalDevice, app->surface);
    uint32_t queueFamilyIndices[] = {indices.graphicsFamily, indices.presentFamily};

    if (indices.graphicsFamily != indices.presentFamily) {
        createInfo.imageSharingMode = VK_SHARING_MODE_CONCURRENT;
        createInfo.queueFamilyIndexCount = 2;
        createInfo.pQueueFamilyIndices = queueFamilyIndices;
    } else {
        createInfo.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE;
        createInfo.queueFamilyIndexCount = 0;
        createInfo.pQueueFamilyIndices = NULL;
    }

    createInfo.preTransform = swapChainSupport.capabilities.currentTransform;
    createInfo.compositeAlpha = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    createInfo.presentMode = presentMode;
    createInfo.clipped = VK_TRUE;
    createInfo.oldSwapchain = VK_NULL_HANDLE;

    if (vkCreateSwapchainKHR(app->device, &createInfo, NULL, &app->swapChain) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create swap chain!\n");
        exit(EXIT_FAILURE);
    }

    vkGetSwapchainImagesKHR(app->device, app->swapChain, &app->swapChainImageCount, NULL);
    app->swapChainImages = malloc(sizeof(VkImage) * app->swapChainImageCount);
    vkGetSwapchainImagesKHR(app->device, app->swapChain, &app->swapChainImageCount, app->swapChainImages);

    app->swapChainImageFormat = surfaceFormat.format;
    app->swapChainExtent = extent;

    freeSwapChainSupportDetails(&swapChainSupport);
}

VkImageView createImageView(SiCompassApplication* app, VkImage image, VkFormat format, VkImageAspectFlags aspectFlags) {
    VkImageViewCreateInfo viewInfo = {0};
    viewInfo.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    viewInfo.image = image;
    viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    viewInfo.format = format;
    viewInfo.subresourceRange.aspectMask = aspectFlags;
    viewInfo.subresourceRange.baseMipLevel = 0;
    viewInfo.subresourceRange.levelCount = 1;
    viewInfo.subresourceRange.baseArrayLayer = 0;
    viewInfo.subresourceRange.layerCount = 1;

    VkImageView imageView;
    if (vkCreateImageView(app->device, &viewInfo, NULL, &imageView) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create image view!\n");
        exit(EXIT_FAILURE);
    }

    return imageView;
}

void createImageViews(SiCompassApplication* app) {
    app->swapChainImageViews = malloc(sizeof(VkImageView) * app->swapChainImageCount);

    for (uint32_t i = 0; i < app->swapChainImageCount; i++) {
        app->swapChainImageViews[i] = createImageView(app, app->swapChainImages[i], app->swapChainImageFormat, VK_IMAGE_ASPECT_COLOR_BIT);
    }
}

VkShaderModule createShaderModule(VkDevice device, const char* code, size_t codeSize) {
    VkShaderModuleCreateInfo createInfo = {0};
    createInfo.sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO;
    createInfo.codeSize = codeSize;
    createInfo.pCode = (const uint32_t*)code;

    VkShaderModule shaderModule;
    if (vkCreateShaderModule(device, &createInfo, NULL, &shaderModule) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create shader module!\n");
        exit(EXIT_FAILURE);
    }

    return shaderModule;
}

uint32_t findMemoryType(VkPhysicalDevice physicalDevice, uint32_t typeFilter, VkMemoryPropertyFlags properties) {
    VkPhysicalDeviceMemoryProperties memProperties;
    vkGetPhysicalDeviceMemoryProperties(physicalDevice, &memProperties);

    for (uint32_t i = 0; i < memProperties.memoryTypeCount; i++) {
        if ((typeFilter & (1 << i)) && (memProperties.memoryTypes[i].propertyFlags & properties) == properties) {
            return i;
        }
    }

    fprintf(stderr, "Failed to find suitable memory type!\n");
    exit(EXIT_FAILURE);
}

void createBuffer(SiCompassApplication* app, VkDeviceSize size, VkBufferUsageFlags usage, 
                  VkMemoryPropertyFlags properties, VkBuffer* buffer, VkDeviceMemory* bufferMemory) {
    VkBufferCreateInfo bufferInfo = {0};
    bufferInfo.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
    bufferInfo.size = size;
    bufferInfo.usage = usage;
    bufferInfo.sharingMode = VK_SHARING_MODE_EXCLUSIVE;

    if (vkCreateBuffer(app->device, &bufferInfo, NULL, buffer) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create buffer!\n");
        exit(EXIT_FAILURE);
    }

    VkMemoryRequirements memRequirements;
    vkGetBufferMemoryRequirements(app->device, *buffer, &memRequirements);

    VkMemoryAllocateInfo allocInfo = {0};
    allocInfo.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocInfo.allocationSize = memRequirements.size;
    allocInfo.memoryTypeIndex = findMemoryType(app->physicalDevice, memRequirements.memoryTypeBits, properties);

    if (vkAllocateMemory(app->device, &allocInfo, NULL, bufferMemory) != VK_SUCCESS) {
        fprintf(stderr, "Failed to allocate buffer memory!\n");
        exit(EXIT_FAILURE);
    }

    vkBindBufferMemory(app->device, *buffer, *bufferMemory, 0);
}

VkCommandBuffer beginSingleTimeCommands(SiCompassApplication* app) {
    VkCommandBufferAllocateInfo allocInfo = {0};
    allocInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocInfo.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocInfo.commandPool = app->commandPool;
    allocInfo.commandBufferCount = 1;

    VkCommandBuffer commandBuffer;
    vkAllocateCommandBuffers(app->device, &allocInfo, &commandBuffer);

    VkCommandBufferBeginInfo beginInfo = {0};
    beginInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    beginInfo.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;

    vkBeginCommandBuffer(commandBuffer, &beginInfo);

    return commandBuffer;
}

void endSingleTimeCommands(SiCompassApplication* app, VkCommandBuffer commandBuffer) {
    vkEndCommandBuffer(commandBuffer);

    VkSubmitInfo submitInfo = {0};
    submitInfo.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
    submitInfo.commandBufferCount = 1;
    submitInfo.pCommandBuffers = &commandBuffer;

    vkQueueSubmit(app->graphicsQueue, 1, &submitInfo, VK_NULL_HANDLE);
    vkQueueWaitIdle(app->graphicsQueue);

    vkFreeCommandBuffers(app->device, app->commandPool, 1, &commandBuffer);
}

void copyBuffer(SiCompassApplication* app, VkBuffer srcBuffer, VkBuffer dstBuffer, VkDeviceSize size) {
    VkCommandBuffer commandBuffer = beginSingleTimeCommands(app);

    VkBufferCopy copyRegion = {0};
    copyRegion.size = size;
    vkCmdCopyBuffer(commandBuffer, srcBuffer, dstBuffer, 1, &copyRegion);

    endSingleTimeCommands(app, commandBuffer);
}

void createImage(SiCompassApplication* app, uint32_t width, uint32_t height, VkFormat format, 
                 VkImageTiling tiling, VkImageUsageFlags usage, VkMemoryPropertyFlags properties, 
                 VkImage* image, VkDeviceMemory* imageMemory) {
    VkImageCreateInfo imageInfo = {0};
    imageInfo.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    imageInfo.imageType = VK_IMAGE_TYPE_2D;
    imageInfo.extent.width = width;
    imageInfo.extent.height = height;
    imageInfo.extent.depth = 1;
    imageInfo.mipLevels = 1;
    imageInfo.arrayLayers = 1;
    imageInfo.format = format;
    imageInfo.tiling = tiling;
    imageInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
    imageInfo.usage = usage;
    imageInfo.samples = VK_SAMPLE_COUNT_1_BIT;
    imageInfo.sharingMode = VK_SHARING_MODE_EXCLUSIVE;

    if (vkCreateImage(app->device, &imageInfo, NULL, image) != VK_SUCCESS) {
        fprintf(stderr, "Failed to create image!\n");
        exit(EXIT_FAILURE);
    }

    VkMemoryRequirements memRequirements;
    vkGetImageMemoryRequirements(app->device, *image, &memRequirements);

    VkMemoryAllocateInfo allocInfo = {0};
    allocInfo.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
    allocInfo.allocationSize = memRequirements.size;
    allocInfo.memoryTypeIndex = findMemoryType(app->physicalDevice, memRequirements.memoryTypeBits, properties);

    if (vkAllocateMemory(app->device, &allocInfo, NULL, imageMemory) != VK_SUCCESS) {
        fprintf(stderr, "Failed to allocate image memory!\n");
        exit(EXIT_FAILURE);
    }

    vkBindImageMemory(app->device, *image, *imageMemory, 0);
}

bool hasStencilComponent(VkFormat format) {
    return format == VK_FORMAT_D32_SFLOAT_S8_UINT || format == VK_FORMAT_D24_UNORM_S8_UINT;
}

void transitionImageLayout(SiCompassApplication* app, VkImage image, VkFormat format, 
                           VkImageLayout oldLayout, VkImageLayout newLayout) {
    VkCommandBuffer commandBuffer = beginSingleTimeCommands(app);

    VkImageMemoryBarrier barrier = {0};
    barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.oldLayout = oldLayout;
    barrier.newLayout = newLayout;
    barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.image = image;
    barrier.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    barrier.subresourceRange.baseMipLevel = 0;
    barrier.subresourceRange.levelCount = 1;
    barrier.subresourceRange.baseArrayLayer = 0;
    barrier.subresourceRange.layerCount = 1;

    VkPipelineStageFlags sourceStage;
    VkPipelineStageFlags destinationStage;

    if (oldLayout == VK_IMAGE_LAYOUT_UNDEFINED && newLayout == VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL) {
        barrier.srcAccessMask = 0;
        barrier.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
        sourceStage = VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT;
        destinationStage = VK_PIPELINE_STAGE_TRANSFER_BIT;
    } else if (oldLayout == VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL && newLayout == VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL) {
        barrier.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
        barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
        sourceStage = VK_PIPELINE_STAGE_TRANSFER_BIT;
        destinationStage = VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT;
    } else {
        fprintf(stderr, "Unsupported layout transition!\n");
        exit(EXIT_FAILURE);
    }

    vkCmdPipelineBarrier(commandBuffer, sourceStage, destinationStage, 0, 0, NULL, 0, NULL, 1, &barrier);

    endSingleTimeCommands(app, commandBuffer);
}

void copyBufferToImage(SiCompassApplication* app, VkBuffer buffer, VkImage image, uint32_t width, uint32_t height) {
    VkCommandBuffer commandBuffer = beginSingleTimeCommands(app);

    VkBufferImageCopy region = {0};
    region.bufferOffset = 0;
    region.bufferRowLength = 0;
    region.bufferImageHeight = 0;
    region.imageSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    region.imageSubresource.mipLevel = 0;
    region.imageSubresource.baseArrayLayer = 0;
    region.imageSubresource.layerCount = 1;
    region.imageOffset = (VkOffset3D){0, 0, 0};
    region.imageExtent = (VkExtent3D){width, height, 1};

    vkCmdCopyBufferToImage(commandBuffer, buffer, image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);

    endSingleTimeCommands(app, commandBuffer);
}

void recordCommandBuffer(SiCompassApplication* app, VkCommandBuffer commandBuffer, uint32_t imageIndex) {
    VkCommandBufferBeginInfo beginInfo = {0};
    beginInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;

    if (vkBeginCommandBuffer(commandBuffer, &beginInfo) != VK_SUCCESS) {
        fprintf(stderr, "Failed to begin recording command buffer!\n");
        exit(EXIT_FAILURE);
    }

    VkRenderPassBeginInfo renderPassInfo = {0};
    renderPassInfo.sType = VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO;
    renderPassInfo.renderPass = app->renderPass;
    renderPassInfo.framebuffer = app->swapChainFramebuffers[imageIndex];
    renderPassInfo.renderArea.offset = (VkOffset2D){0, 0};
    renderPassInfo.renderArea.extent = app->swapChainExtent;

    VkClearValue clearValues[2];
    clearValues[0].color.float32[0] = 0.0f;
    clearValues[0].color.float32[1] = 0.0f;
    clearValues[0].color.float32[2] = 0.0f;
    clearValues[0].color.float32[3] = 1.0f;

    clearValues[1].depthStencil.depth = 1.0f;
    clearValues[1].depthStencil.stencil = 0;

    renderPassInfo.clearValueCount = 2;
    renderPassInfo.pClearValues = clearValues;

    vkCmdBeginRenderPass(commandBuffer, &renderPassInfo, VK_SUBPASS_CONTENTS_INLINE);

    drawImage(app, commandBuffer);
    drawBackground(app, commandBuffer);
    drawText(app, commandBuffer);

    vkCmdEndRenderPass(commandBuffer);

    if (vkEndCommandBuffer(commandBuffer) != VK_SUCCESS) {
        fprintf(stderr, "Failed to record command buffer!\n");
        exit(EXIT_FAILURE);
    }
}

void cleanupSwapChain(SiCompassApplication* app) {
    vkDestroyImageView(app->device, app->depthImageView, NULL);
    vkDestroyImage(app->device, app->depthImage, NULL);
    vkFreeMemory(app->device, app->depthImageMemory, NULL);

    for (uint32_t i = 0; i < app->swapChainImageCount; i++) {
        vkDestroyFramebuffer(app->device, app->swapChainFramebuffers[i], NULL);
    }

    for (uint32_t i = 0; i < app->swapChainImageCount; i++) {
        vkDestroyImageView(app->device, app->swapChainImageViews[i], NULL);
    }

    vkDestroySwapchainKHR(app->device, app->swapChain, NULL);

    free(app->swapChainFramebuffers);
    free(app->swapChainImageViews);
    free(app->swapChainImages);
}

void recreateSwapChain(SiCompassApplication* app) {
    int width = 0, height = 0;
    SDL_GetWindowSizeInPixels(app->window, &width, &height);
    while (width == 0 || height == 0) {
        SDL_GetWindowSizeInPixels(app->window, &width, &height);
        SDL_WaitEvent(NULL);
    }

    vkDeviceWaitIdle(app->device);

    cleanupSwapChain(app);

    createSwapChain(app);
    createImageViews(app);
    createDepthResources(app);
    createFramebuffers(app);
}

void drawFrame(SiCompassApplication* app) {
    vkWaitForFences(app->device, 1, &app->inFlightFences[app->currentFrame], VK_TRUE, UINT64_MAX);

    uint32_t imageIndex;
    VkResult result = vkAcquireNextImageKHR(app->device, app->swapChain, UINT64_MAX, 
                                           app->imageAvailableSemaphores[app->currentFrame], VK_NULL_HANDLE, &imageIndex);

    if (result == VK_ERROR_OUT_OF_DATE_KHR) {
        recreateSwapChain(app);
        return;
    } else if (result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR) {
        fprintf(stderr, "Failed to acquire swap chain image!\n");
        exit(EXIT_FAILURE);
    }

    updateUniformBuffer(app, app->currentFrame);

    vkResetFences(app->device, 1, &app->inFlightFences[app->currentFrame]);

    vkResetCommandBuffer(app->commandBuffers[app->currentFrame], 0);
    recordCommandBuffer(app, app->commandBuffers[app->currentFrame], imageIndex);

    VkSubmitInfo submitInfo = {0};
    submitInfo.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;

    VkSemaphore waitSemaphores[] = {app->imageAvailableSemaphores[app->currentFrame]};
    VkPipelineStageFlags waitStages[] = {VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT};
    submitInfo.waitSemaphoreCount = 1;
    submitInfo.pWaitSemaphores = waitSemaphores;
    submitInfo.pWaitDstStageMask = waitStages;

    submitInfo.commandBufferCount = 1;
    submitInfo.pCommandBuffers = &app->commandBuffers[app->currentFrame];

    VkSemaphore signalSemaphores[] = {app->renderFinishedSemaphores[app->currentFrame]};
    submitInfo.signalSemaphoreCount = 1;
    submitInfo.pSignalSemaphores = signalSemaphores;

    if (vkQueueSubmit(app->graphicsQueue, 1, &submitInfo, app->inFlightFences[app->currentFrame]) != VK_SUCCESS) {
        fprintf(stderr, "Failed to submit draw command buffer!\n");
        exit(EXIT_FAILURE);
    }

    VkPresentInfoKHR presentInfo = {0};
    presentInfo.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR;
    presentInfo.waitSemaphoreCount = 1;
    presentInfo.pWaitSemaphores = signalSemaphores;

    VkSwapchainKHR swapChains[] = {app->swapChain};
    presentInfo.swapchainCount = 1;
    presentInfo.pSwapchains = swapChains;
    presentInfo.pImageIndices = &imageIndex;

    result = vkQueuePresentKHR(app->presentQueue, &presentInfo);

    if (result == VK_ERROR_OUT_OF_DATE_KHR || result == VK_SUBOPTIMAL_KHR || app->framebufferResized) {
        app->framebufferResized = false;
        recreateSwapChain(app);
    } else if (result != VK_SUCCESS) {
        fprintf(stderr, "Failed to present swap chain image!\n");
        exit(EXIT_FAILURE);
    }

    app->currentFrame = (app->currentFrame + 1) % MAX_FRAMES_IN_FLIGHT;
}


void initVulkan(SiCompassApplication* app) {
    createInstance(app);
    setupDebugMessenger(app);
    createSurface(app);
    pickPhysicalDevice(app);
    createLogicalDevice(app);
    createSwapChain(app);
    createImageViews(app);

    // Image
    createRenderPass(app);
    createImageDescriptorSetLayout(app);
    createImagePipeline(app);
    createCommandPool(app);
    createDepthResources(app);
    createFramebuffers(app);
    createTextureImage(app);
    createTextureImageView(app);
    createTextureSampler(app);
    createImageVertexBuffer(app);
    createImageIndexBuffer(app);
    createUniformBuffers(app);
    createImageDescriptorPool(app);
    createImageDescriptorSets(app);
    createCommandBuffers(app);
    createSyncObjects(app);

    // Text
    initFreeType(app);
    createFontAtlas(app);
    createFontAtlasView(app);
    createFontAtlasSampler(app);
    createTextVertexBuffer(app);
    createBackgroundVertexBuffer(app);
    createTextDescriptorSetLayout(app);
    createTextDescriptorPool(app);
    createTextDescriptorSets(app);
    createTextPipeline(app);
    createBackgroundPipeline(app);
}

void mainLoop(SiCompassApplication* app) {
    startApp(app);

    vkDeviceWaitIdle(app->device);
}

void cleanup(SiCompassApplication* app) {
    cleanupSwapChain(app);

    vkDestroyPipeline(app->device, app->graphicsPipeline, NULL);
    vkDestroyPipelineLayout(app->device, app->pipelineLayout, NULL);
    vkDestroyRenderPass(app->device, app->renderPass, NULL);

    for (size_t i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        vkDestroyBuffer(app->device, app->uniformBuffers[i], NULL);
        vkFreeMemory(app->device, app->uniformBuffersMemory[i], NULL);
    }

    vkDestroyDescriptorPool(app->device, app->descriptorPool, NULL);

    cleanupTextureResources(app);

    vkDestroyDescriptorSetLayout(app->device, app->descriptorSetLayout, NULL);

    vkDestroyBuffer(app->device, app->indexBuffer, NULL);
    vkFreeMemory(app->device, app->indexBufferMemory, NULL);

    vkDestroyBuffer(app->device, app->vertexBuffer, NULL);
    vkFreeMemory(app->device, app->vertexBufferMemory, NULL);

    for (size_t i = 0; i < MAX_FRAMES_IN_FLIGHT; i++) {
        vkDestroySemaphore(app->device, app->renderFinishedSemaphores[i], NULL);
        vkDestroySemaphore(app->device, app->imageAvailableSemaphores[i], NULL);
        vkDestroyFence(app->device, app->inFlightFences[i], NULL);
    }

    vkDestroyCommandPool(app->device, app->commandPool, NULL);

    vkDestroyDevice(app->device, NULL);

    if (enableValidationLayers) {
        DestroyDebugUtilsMessengerEXT(app->instance, app->debugMessenger, NULL);
    }

    vkDestroySurfaceKHR(app->instance, app->surface, NULL);
    vkDestroyInstance(app->instance, NULL);

    SDL_DestroyWindow(app->window);
    SDL_Quit();

    cleanupFontRenderer(app);
}

void run(SiCompassApplication* app) {
    initWindow(app);
    initVulkan(app);
    mainLoop(app);
    cleanup(app);
}

int main(void) {
    SiCompassApplication app = {0};
    siCompassApplicationInit(&app);

    run(&app);

    return EXIT_SUCCESS;
}