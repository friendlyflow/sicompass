#version 450

// Color-glyph (emoji) fragment shader. The atlas holds premultiplied RGBA
// texels rasterized from a color bitmap font; the per-vertex color from the
// text pipeline is intentionally ignored so emoji keep their own colors.
layout(binding = 0) uniform sampler2D emojiAtlas;

layout(location = 0) in vec2 fragTexCoord;
layout(location = 1) in vec3 fragColor;  // unused for color glyphs

layout(location = 0) out vec4 outColor;

void main() {
    outColor = texture(emojiAtlas, fragTexCoord);
}
