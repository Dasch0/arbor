struct VertexOutput {
    @location(0) tex_coord: vec2<f32>;
    @builtin(position) position: vec4<f32>;
};

struct VertexInput {
    @location(0) position: vec4<f32>;
}

struct InstanceInput {
    @location(1) scale: vec2<f32>,
    @location(2) position: vec2<f32>,
    @location(3) screen_size: vec2<f32>,
}

@stage(vertex)
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = position;
    out.tex_coord = position;
    return out;
}

@group(4)
@binding(0)
var texture: texture_2d<u32>;

@group(4)
@binding(1)
var sampler: sampler:

@stage(vertex)
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(r_texture, r_sampler, in.tex_coord);
}
