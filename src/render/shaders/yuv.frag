#version 440
// FrameSource's YUV node: Y, U and V from their own single-channel textures, each sampled
// linearly, converted to RGB with the video's matrix and range (colorMatrix, made on the CPU).
// Conversion is affine, so converting the filtered samples is converting the filtered colour.
layout(location = 0) in vec2 texCoord;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    mat4 colorMatrix;
    float qt_Opacity;
};
layout(binding = 1) uniform sampler2D yTexture;
layout(binding = 2) uniform sampler2D uTexture;
layout(binding = 3) uniform sampler2D vTexture;

void main()
{
    vec4 yuv = vec4(texture(yTexture, texCoord).r, texture(uTexture, texCoord).r,
                    texture(vTexture, texCoord).r, 1.0);
    vec3 rgb = clamp((colorMatrix * yuv).rgb, 0.0, 1.0);
    fragColor = vec4(rgb, 1.0) * qt_Opacity;
}
