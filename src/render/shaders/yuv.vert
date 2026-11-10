#version 440
// FrameSource's YUV node: a textured quad, as Qt's own texture material draws it.
layout(location = 0) in vec4 qt_VertexPosition;
layout(location = 1) in vec2 qt_VertexTexCoord;
layout(location = 0) out vec2 texCoord;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    mat4 colorMatrix;
    float qt_Opacity;
};
out gl_PerVertex { vec4 gl_Position; };

void main()
{
    texCoord = qt_VertexTexCoord;
    gl_Position = qt_Matrix * qt_VertexPosition;
}
