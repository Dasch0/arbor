#version 450

layout(location = 0) in vec4 a_Pos;
layout(location = 1) in vec2 a_TexCoord;
layout(location = 0) out vec2 v_TexCoord;
layout(push_constant) uniform fragment{
    int index;
} pc;
layout(set = 0, binding = 0) buffer viewProj {
  mat4 view_proj[];
};
layout(set = 0, binding = 3) buffer Model {
  mat4 model[];
};

void main() {
  v_TexCoord = a_TexCoord;
  gl_Position = view_proj[pc.index] * model[gl_InstanceIndex] * a_Pos;
}    

