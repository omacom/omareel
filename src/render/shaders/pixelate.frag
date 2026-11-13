#version 440
// The preview's pixelated blur region: each of `cells` takes one sample of the mipmapped
// source at the level whose texels are about a cell big, i.e. the cell's average. The export
// bakes the same grid into the recording's pixels (crates/omareel_core/src/export/redact.rs).
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec2 cells;
};
layout(binding = 1) uniform sampler2D source;

void main()
{
    vec2 cell = (floor(qt_TexCoord0 * cells) + 0.5) / cells;
    vec2 size = vec2(textureSize(source, 0));
    float lod = log2(max(1.0, max(size.x / cells.x, size.y / cells.y)));
    fragColor = vec4(textureLod(source, cell, lod).rgb, 1.0) * qt_Opacity;
}
