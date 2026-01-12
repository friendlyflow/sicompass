#version 450

layout(location = 0) in vec4 fragColor;
layout(location = 1) in vec2 fragPos;
layout(location = 2) in vec2 fragCornerRadius;
layout(location = 3) in vec2 fragRectSize;

layout(location = 0) out vec4 outColor;

float roundedBoxSDF(vec2 pos, vec2 size, float radius) {
    vec2 q = abs(pos) - size + radius;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radius;
}

void main() {
    // Calculate position relative to rectangle center
    vec2 center = fragRectSize * 0.5;
    vec2 localPos = fragPos - center;

    // Calculate distance to rounded rectangle edge
    float distance = roundedBoxSDF(localPos, fragRectSize * 0.5, fragCornerRadius.x);

    // Smooth anti-aliasing
    float alpha = 1.0 - smoothstep(-1.0, 1.0, distance);

    outColor = vec4(fragColor.rgb, fragColor.a * alpha);
}
