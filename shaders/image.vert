#version 450

layout(location = 0) in vec2 inPosition;
layout(location = 1) in vec2 inTexCoord;

layout(push_constant) uniform PushConstants {
    float screenWidth;
    float screenHeight;
} pushConstants;

layout(location = 0) out vec2 fragTexCoord;

void main() {
    float x = (inPosition.x / pushConstants.screenWidth) * 2.0 - 1.0;
    float y = (inPosition.y / pushConstants.screenHeight) * 2.0 - 1.0;

    gl_Position = vec4(x, y, 0.0, 1.0);
    fragTexCoord = inTexCoord;
}
