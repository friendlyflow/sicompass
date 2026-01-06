#version 450

layout(binding = 0) uniform sampler2D fontAtlas;

layout(location = 0) in vec2 fragTexCoord;
layout(location = 1) in vec3 fragColor;  // ADD THIS

layout(location = 0) out vec4 outColor;

void main() {
    float alpha = texture(fontAtlas, fragTexCoord).r;
    outColor = vec4(fragColor, alpha);
}