#version 450

layout(location = 0) in vec2 inPosition;
layout(location = 1) in vec4 inColor;
layout(location = 2) in vec2 inCornerRadius;
layout(location = 3) in vec2 inRectSize;
layout(location = 4) in vec2 inRectOrigin;

layout(push_constant) uniform PushConstants {
    float screenWidth;
    float screenHeight;
} pushConstants;

layout(location = 0) out vec4 fragColor;
layout(location = 1) out vec2 fragPos;
layout(location = 2) out vec2 fragCornerRadius;
layout(location = 3) out vec2 fragRectSize;

void main() {
    float x = (inPosition.x / pushConstants.screenWidth) * 2.0 - 1.0;
    float y = (inPosition.y / pushConstants.screenHeight) * 2.0 - 1.0;

    gl_Position = vec4(x, y, 0.0, 1.0);
    fragColor = inColor;
    fragPos = inPosition - inRectOrigin;  // Convert to local coordinates
    fragCornerRadius = inCornerRadius;
    fragRectSize = inRectSize;
}
