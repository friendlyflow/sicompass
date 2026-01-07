#version 450

layout(location = 0) in vec3 inPosition;
layout(location = 1) in vec2 inTexCoord;
layout(location = 2) in vec3 inColor;

layout(push_constant) uniform PushConstants {
    float screenWidth;
    float screenHeight;
} pushConstants;

layout(location = 0) out vec2 fragTexCoord;
layout(location = 1) out vec3 fragColor;

void main() {
    float x = (inPosition.x / pushConstants.screenWidth) * 2.0 - 1.0;
    float y = (inPosition.y / pushConstants.screenHeight) * 2.0 - 1.0;
    // REMOVED: y = -y;  // This was flipping the text upside down
    
    gl_Position = vec4(x, y, inPosition.z, 1.0);
    fragTexCoord = inTexCoord;
    fragColor = inColor;
}