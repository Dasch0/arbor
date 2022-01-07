struct VertexOutput {
    [[builtin(position)]] position: vec4<f32>;
};

[[block]]
struct PushConstants {
    color: vec4<f32>;
    transform: mat4x4<f32>;
};
var<push_constant> pc: PushConstants;

[[stage(vertex)]]
fn vs_main(
    [[location(0)]] position: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = pc.transform * position;
    return out;
}

[[stage(fragment)]]
fn fs_main(in: VertexOutput) -> [[location(0)]] vec4<f32> {
    return pc.color;
}
