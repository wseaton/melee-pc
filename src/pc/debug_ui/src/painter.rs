use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::ptr;

use egui::epaint::{ImageDelta, Primitive, Vertex};
use egui::{ClippedPrimitive, ImageData, TextureId, TexturesDelta};

use crate::gpu::*;

const SHADER: &str = include_str!("shader.wgsl");

#[derive(Debug)]
pub enum PainterError {
    Create(&'static str),
    UnknownTexture(TextureId),
}

impl fmt::Display for PainterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create(what) => write!(f, "failed to create {what}"),
            Self::UnknownTexture(id) => write!(f, "mesh references unknown texture {id:?}"),
        }
    }
}

impl std::error::Error for PainterError {}

macro_rules! handle {
    ($name:ident, $raw:ty, $release:ident, $what:literal) => {
        struct $name($raw);

        impl $name {
            fn new(raw: $raw) -> Result<Self, PainterError> {
                if raw.is_null() {
                    Err(PainterError::Create($what))
                } else {
                    Ok(Self(raw))
                }
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                unsafe { $release(self.0) }
            }
        }

        unsafe impl Send for $name {}
    };
}

handle!(
    ShaderModule,
    WGPUShaderModule,
    wgpuShaderModuleRelease,
    "shader module"
);
handle!(
    BindGroupLayout,
    WGPUBindGroupLayout,
    wgpuBindGroupLayoutRelease,
    "bind group layout"
);
handle!(
    PipelineLayout,
    WGPUPipelineLayout,
    wgpuPipelineLayoutRelease,
    "pipeline layout"
);
handle!(
    RenderPipeline,
    WGPURenderPipeline,
    wgpuRenderPipelineRelease,
    "render pipeline"
);
handle!(Buffer, WGPUBuffer, wgpuBufferRelease, "buffer");
handle!(BindGroup, WGPUBindGroup, wgpuBindGroupRelease, "bind group");
handle!(Sampler, WGPUSampler, wgpuSamplerRelease, "sampler");
handle!(Texture, WGPUTexture, wgpuTextureRelease, "texture");
handle!(
    TextureView,
    WGPUTextureView,
    wgpuTextureViewRelease,
    "texture view"
);

fn string_view(s: &str) -> WGPUStringView {
    WGPUStringView {
        data: s.as_ptr().cast(),
        length: s.len(),
    }
}

pub struct Target {
    pub device: WGPUDevice,
    pub queue: WGPUQueue,
    pub pass: WGPURenderPassEncoder,
    pub format: WGPUTextureFormat,
    pub width: u32,
    pub height: u32,
}

pub struct Frame {
    pub textures: TexturesDelta,
    pub primitives: Vec<ClippedPrimitive>,
    pub pixels_per_point: f32,
}

impl Drop for Frame {
    fn drop(&mut self) {
        self.textures.clear();
    }
}

struct GrowBuffer {
    buffer: Buffer,
    capacity: u64,
    usage: WGPUBufferUsage,
}

impl GrowBuffer {
    fn new(
        device: WGPUDevice,
        usage: WGPUBufferUsage,
        capacity: u64,
    ) -> Result<Self, PainterError> {
        let desc = WGPUBufferDescriptor {
            usage: usage | WGPUBufferUsage_CopyDst,
            size: capacity,
            ..Default::default()
        };
        let buffer = Buffer::new(unsafe { wgpuDeviceCreateBuffer(device, &desc) })?;
        Ok(Self {
            buffer,
            capacity,
            usage,
        })
    }

    fn upload(&mut self, target: &Target, bytes: &[u8]) -> Result<(), PainterError> {
        let needed = bytes.len() as u64;
        if needed > self.capacity {
            *self = Self::new(target.device, self.usage, needed.next_power_of_two())?;
        }
        unsafe {
            wgpuQueueWriteBuffer(
                target.queue,
                self.buffer.0,
                0,
                bytes.as_ptr().cast(),
                bytes.len(),
            )
        };
        Ok(())
    }
}

struct GpuTexture {
    texture: Texture,
    _view: TextureView,
    bind_group: BindGroup,
}

struct Draw {
    scissor: [u32; 4],
    texture: TextureId,
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
}

pub struct Painter {
    shader: ShaderModule,
    pipeline_layout: PipelineLayout,
    texture_layout: BindGroupLayout,
    pipeline: Option<(WGPUTextureFormat, RenderPipeline)>,
    uniforms: Buffer,
    uniform_bind_group: BindGroup,
    sampler: Sampler,
    vertices: GrowBuffer,
    indices: GrowBuffer,
    textures: HashMap<TextureId, GpuTexture>,
    retained: Vec<ClippedPrimitive>,
    pixels_per_point: f32,
}

impl Painter {
    pub fn new(device: WGPUDevice) -> Result<Self, PainterError> {
        let mut wgsl = WGPUShaderSourceWGSL {
            code: string_view(SHADER),
            ..Default::default()
        };
        wgsl.chain.sType = WGPUSType_ShaderSourceWGSL;
        let shader_desc = WGPUShaderModuleDescriptor {
            nextInChain: (&raw mut wgsl.chain),
            ..Default::default()
        };
        let shader =
            ShaderModule::new(unsafe { wgpuDeviceCreateShaderModule(device, &shader_desc) })?;

        let uniform_entries = [WGPUBindGroupLayoutEntry {
            binding: 0,
            visibility: WGPUShaderStage_Vertex,
            buffer: WGPUBufferBindingLayout {
                type_: WGPUBufferBindingType_Uniform,
                ..Default::default()
            },
            ..Default::default()
        }];
        let uniform_layout = bind_group_layout(device, &uniform_entries)?;

        let texture_entries = [
            WGPUBindGroupLayoutEntry {
                binding: 0,
                visibility: WGPUShaderStage_Fragment,
                texture: WGPUTextureBindingLayout {
                    sampleType: WGPUTextureSampleType_Float,
                    viewDimension: WGPUTextureViewDimension_2D,
                    ..Default::default()
                },
                ..Default::default()
            },
            WGPUBindGroupLayoutEntry {
                binding: 1,
                visibility: WGPUShaderStage_Fragment,
                sampler: WGPUSamplerBindingLayout {
                    type_: WGPUSamplerBindingType_Filtering,
                    ..Default::default()
                },
                ..Default::default()
            },
        ];
        let texture_layout = bind_group_layout(device, &texture_entries)?;

        let layouts = [uniform_layout.0, texture_layout.0];
        let pipeline_layout_desc = WGPUPipelineLayoutDescriptor {
            bindGroupLayoutCount: layouts.len(),
            bindGroupLayouts: layouts.as_ptr(),
            ..Default::default()
        };
        let pipeline_layout = PipelineLayout::new(unsafe {
            wgpuDeviceCreatePipelineLayout(device, &pipeline_layout_desc)
        })?;

        let uniforms_desc = WGPUBufferDescriptor {
            usage: WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst,
            size: size_of::<[f32; 4]>() as u64,
            ..Default::default()
        };
        let uniforms = Buffer::new(unsafe { wgpuDeviceCreateBuffer(device, &uniforms_desc) })?;
        let uniform_bind_entries = [WGPUBindGroupEntry {
            binding: 0,
            buffer: uniforms.0,
            size: uniforms_desc.size,
            ..Default::default()
        }];
        let uniform_bind_group = bind_group(device, &uniform_layout, &uniform_bind_entries)?;

        let sampler_desc = WGPUSamplerDescriptor {
            addressModeU: WGPUAddressMode_ClampToEdge,
            addressModeV: WGPUAddressMode_ClampToEdge,
            addressModeW: WGPUAddressMode_ClampToEdge,
            magFilter: WGPUFilterMode_Linear,
            minFilter: WGPUFilterMode_Linear,
            mipmapFilter: WGPUMipmapFilterMode_Nearest,
            lodMaxClamp: 32.0,
            maxAnisotropy: 1,
            ..Default::default()
        };
        let sampler = Sampler::new(unsafe { wgpuDeviceCreateSampler(device, &sampler_desc) })?;

        Ok(Self {
            shader,
            pipeline_layout,
            texture_layout,
            pipeline: None,
            uniforms,
            uniform_bind_group,
            sampler,
            vertices: GrowBuffer::new(device, WGPUBufferUsage_Vertex, 1 << 16)?,
            indices: GrowBuffer::new(device, WGPUBufferUsage_Index, 1 << 16)?,
            textures: HashMap::new(),
            retained: Vec::new(),
            pixels_per_point: 1.0,
        })
    }

    pub fn paint(&mut self, target: &Target, newer: Option<Frame>) -> Result<(), PainterError> {
        let Some(mut frame) = newer else {
            return self.draw_retained(target);
        };
        for (id, deltas) in &frame.textures.set {
            for delta in deltas {
                self.set_texture(target, *id, delta)?;
            }
        }
        self.retained = std::mem::take(&mut frame.primitives);
        self.pixels_per_point = frame.pixels_per_point;
        let result = self.draw_retained(target);
        for id in &frame.textures.free {
            self.textures.remove(id);
        }
        result
    }

    fn draw_retained(&mut self, target: &Target) -> Result<(), PainterError> {
        let primitives = std::mem::take(&mut self.retained);
        let result = self.draw(target, &primitives, self.pixels_per_point);
        self.retained = primitives;
        result
    }

    fn draw(
        &mut self,
        target: &Target,
        primitives: &[ClippedPrimitive],
        pixels_per_point: f32,
    ) -> Result<(), PainterError> {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut draws = Vec::new();
        for ClippedPrimitive {
            clip_rect,
            primitive,
        } in primitives
        {
            let Primitive::Mesh(mesh) = primitive else {
                continue;
            };
            let Some(scissor) = scissor(clip_rect, pixels_per_point, target) else {
                continue;
            };
            if mesh.indices.is_empty() {
                continue;
            }
            draws.push(Draw {
                scissor,
                texture: mesh.texture_id,
                first_index: indices.len() as u32,
                index_count: mesh.indices.len() as u32,
                base_vertex: vertices.len() as i32,
            });
            vertices.extend_from_slice(&mesh.vertices);
            indices.extend_from_slice(&mesh.indices);
        }
        if draws.is_empty() {
            return Ok(());
        }

        self.vertices
            .upload(target, bytemuck::cast_slice(&vertices))?;
        self.indices
            .upload(target, bytemuck::cast_slice(&indices))?;
        let globals = [
            target.width as f32 / pixels_per_point,
            target.height as f32 / pixels_per_point,
            0.0,
            0.0,
        ];
        let globals_bytes: &[u8] = bytemuck::cast_slice(&globals);
        unsafe {
            wgpuQueueWriteBuffer(
                target.queue,
                self.uniforms.0,
                0,
                globals_bytes.as_ptr().cast(),
                globals_bytes.len(),
            );
        }

        let pipeline = self.pipeline_for(target)?;
        let pass = target.pass;
        unsafe {
            wgpuRenderPassEncoderSetPipeline(pass, pipeline);
            wgpuRenderPassEncoderSetBindGroup(pass, 0, self.uniform_bind_group.0, 0, ptr::null());
            wgpuRenderPassEncoderSetVertexBuffer(pass, 0, self.vertices.buffer.0, 0, u64::MAX);
            wgpuRenderPassEncoderSetIndexBuffer(
                pass,
                self.indices.buffer.0,
                WGPUIndexFormat_Uint32,
                0,
                u64::MAX,
            );
        }
        for draw in &draws {
            let texture = self
                .textures
                .get(&draw.texture)
                .ok_or(PainterError::UnknownTexture(draw.texture))?;
            let [x, y, width, height] = draw.scissor;
            unsafe {
                wgpuRenderPassEncoderSetScissorRect(pass, x, y, width, height);
                wgpuRenderPassEncoderSetBindGroup(pass, 1, texture.bind_group.0, 0, ptr::null());
                wgpuRenderPassEncoderDrawIndexed(
                    pass,
                    draw.index_count,
                    1,
                    draw.first_index,
                    draw.base_vertex,
                    0,
                );
            }
        }
        Ok(())
    }

    fn pipeline_for(&mut self, target: &Target) -> Result<WGPURenderPipeline, PainterError> {
        if let Some((format, pipeline)) = &self.pipeline
            && *format == target.format
        {
            return Ok(pipeline.0);
        }

        let attributes = [
            vertex_attribute(0, WGPUVertexFormat_Float32x2, 0),
            vertex_attribute(1, WGPUVertexFormat_Float32x2, 8),
            vertex_attribute(2, WGPUVertexFormat_Unorm8x4, 16),
        ];
        let vertex_buffers = [WGPUVertexBufferLayout {
            stepMode: WGPUVertexStepMode_Vertex,
            arrayStride: size_of::<Vertex>() as u64,
            attributeCount: attributes.len(),
            attributes: attributes.as_ptr(),
            ..Default::default()
        }];
        let blend = WGPUBlendState {
            color: WGPUBlendComponent {
                operation: WGPUBlendOperation_Add,
                srcFactor: WGPUBlendFactor_One,
                dstFactor: WGPUBlendFactor_OneMinusSrcAlpha,
            },
            alpha: WGPUBlendComponent {
                operation: WGPUBlendOperation_Add,
                srcFactor: WGPUBlendFactor_OneMinusDstAlpha,
                dstFactor: WGPUBlendFactor_One,
            },
        };
        let targets = [WGPUColorTargetState {
            format: target.format,
            blend: &blend,
            writeMask: WGPUColorWriteMask_All,
            ..Default::default()
        }];
        let fragment_entry = if is_srgb(target.format) {
            "fs_linear"
        } else {
            "fs_gamma"
        };
        let fragment = WGPUFragmentState {
            module: self.shader.0,
            entryPoint: string_view(fragment_entry),
            targetCount: targets.len(),
            targets: targets.as_ptr(),
            ..Default::default()
        };
        let desc = WGPURenderPipelineDescriptor {
            label: string_view("egui debug ui"),
            layout: self.pipeline_layout.0,
            vertex: WGPUVertexState {
                module: self.shader.0,
                entryPoint: string_view("vs_main"),
                bufferCount: vertex_buffers.len(),
                buffers: vertex_buffers.as_ptr(),
                ..Default::default()
            },
            primitive: WGPUPrimitiveState {
                topology: WGPUPrimitiveTopology_TriangleList,
                frontFace: WGPUFrontFace_CCW,
                cullMode: WGPUCullMode_None,
                ..Default::default()
            },
            multisample: WGPUMultisampleState {
                count: 1,
                mask: u32::MAX,
                ..Default::default()
            },
            fragment: &fragment,
            ..Default::default()
        };
        let pipeline =
            RenderPipeline::new(unsafe { wgpuDeviceCreateRenderPipeline(target.device, &desc) })?;
        let raw = pipeline.0;
        self.pipeline = Some((target.format, pipeline));
        Ok(raw)
    }

    fn set_texture(
        &mut self,
        target: &Target,
        id: TextureId,
        delta: &ImageDelta,
    ) -> Result<(), PainterError> {
        let ImageData::Color(image) = &delta.image;
        let [width, height] = [image.size[0] as u32, image.size[1] as u32];
        let origin = delta.pos.map_or([0, 0], |[x, y]| [x as u32, y as u32]);

        if delta.pos.is_none() {
            self.textures
                .insert(id, self.create_texture(target, width, height)?);
        }
        let texture = self
            .textures
            .get(&id)
            .ok_or(PainterError::UnknownTexture(id))?;

        let destination = WGPUTexelCopyTextureInfo {
            texture: texture.texture.0,
            mipLevel: 0,
            origin: WGPUOrigin3D {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: WGPUTextureAspect_All,
        };
        let layout = WGPUTexelCopyBufferLayout {
            offset: 0,
            bytesPerRow: 4 * width,
            rowsPerImage: height,
        };
        let extent = WGPUExtent3D {
            width,
            height,
            depthOrArrayLayers: 1,
        };
        let pixels: &[u8] = bytemuck::cast_slice(&image.pixels);
        unsafe {
            wgpuQueueWriteTexture(
                target.queue,
                &destination,
                pixels.as_ptr().cast::<c_void>(),
                pixels.len(),
                &layout,
                &extent,
            );
        }
        Ok(())
    }

    fn create_texture(
        &self,
        target: &Target,
        width: u32,
        height: u32,
    ) -> Result<GpuTexture, PainterError> {
        let desc = WGPUTextureDescriptor {
            label: string_view("egui texture"),
            usage: WGPUTextureUsage_TextureBinding | WGPUTextureUsage_CopyDst,
            dimension: WGPUTextureDimension_2D,
            size: WGPUExtent3D {
                width,
                height,
                depthOrArrayLayers: 1,
            },
            format: WGPUTextureFormat_RGBA8Unorm,
            mipLevelCount: 1,
            sampleCount: 1,
            ..Default::default()
        };
        let texture = Texture::new(unsafe { wgpuDeviceCreateTexture(target.device, &desc) })?;
        let view = TextureView::new(unsafe { wgpuTextureCreateView(texture.0, ptr::null()) })?;
        let entries = [
            WGPUBindGroupEntry {
                binding: 0,
                textureView: view.0,
                ..Default::default()
            },
            WGPUBindGroupEntry {
                binding: 1,
                sampler: self.sampler.0,
                ..Default::default()
            },
        ];
        let bind_group = bind_group(target.device, &self.texture_layout, &entries)?;
        Ok(GpuTexture {
            texture,
            _view: view,
            bind_group,
        })
    }
}

fn bind_group_layout(
    device: WGPUDevice,
    entries: &[WGPUBindGroupLayoutEntry],
) -> Result<BindGroupLayout, PainterError> {
    let desc = WGPUBindGroupLayoutDescriptor {
        entryCount: entries.len(),
        entries: entries.as_ptr(),
        ..Default::default()
    };
    BindGroupLayout::new(unsafe { wgpuDeviceCreateBindGroupLayout(device, &desc) })
}

fn bind_group(
    device: WGPUDevice,
    layout: &BindGroupLayout,
    entries: &[WGPUBindGroupEntry],
) -> Result<BindGroup, PainterError> {
    let desc = WGPUBindGroupDescriptor {
        layout: layout.0,
        entryCount: entries.len(),
        entries: entries.as_ptr(),
        ..Default::default()
    };
    BindGroup::new(unsafe { wgpuDeviceCreateBindGroup(device, &desc) })
}

fn vertex_attribute(location: u32, format: WGPUVertexFormat, offset: u64) -> WGPUVertexAttribute {
    WGPUVertexAttribute {
        format,
        offset,
        shaderLocation: location,
        ..Default::default()
    }
}

fn is_srgb(format: WGPUTextureFormat) -> bool {
    format == WGPUTextureFormat_BGRA8UnormSrgb || format == WGPUTextureFormat_RGBA8UnormSrgb
}

fn scissor(clip: &egui::Rect, pixels_per_point: f32, target: &Target) -> Option<[u32; 4]> {
    let clamp =
        |value: f32, max: u32| (value * pixels_per_point).round().clamp(0.0, max as f32) as u32;
    let min_x = clamp(clip.min.x, target.width);
    let min_y = clamp(clip.min.y, target.height);
    let max_x = clamp(clip.max.x, target.width);
    let max_y = clamp(clip.max.y, target.height);
    (max_x > min_x && max_y > min_y).then(|| [min_x, min_y, max_x - min_x, max_y - min_y])
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2};

    use crate::painter::{Target, scissor};

    fn target(width: u32, height: u32) -> Target {
        Target {
            device: std::ptr::null_mut(),
            queue: std::ptr::null_mut(),
            pass: std::ptr::null_mut(),
            format: 0,
            width,
            height,
        }
    }

    #[test]
    fn scissor_scales_points_to_pixels() {
        let clip = Rect::from_min_max(pos2(10.0, 20.0), pos2(110.0, 70.0));
        assert_eq!(
            scissor(&clip, 2.0, &target(1000, 1000)),
            Some([20, 40, 200, 100])
        );
    }

    #[test]
    fn scissor_clamps_to_target() {
        let clip = Rect::from_min_max(pos2(-50.0, -50.0), pos2(5000.0, 5000.0));
        assert_eq!(
            scissor(&clip, 1.0, &target(640, 480)),
            Some([0, 0, 640, 480])
        );
    }

    #[test]
    fn scissor_rejects_empty_and_offscreen() {
        let t = target(640, 480);
        assert_eq!(
            scissor(
                &Rect::from_min_max(pos2(5.0, 5.0), pos2(5.0, 50.0)),
                1.0,
                &t
            ),
            None
        );
        assert_eq!(
            scissor(
                &Rect::from_min_max(pos2(700.0, 0.0), pos2(800.0, 50.0)),
                1.0,
                &t
            ),
            None
        );
        assert_eq!(scissor(&Rect::NOTHING, 1.0, &t), None);
    }

    #[test]
    fn scissor_handles_infinite_clip() {
        assert_eq!(
            scissor(&Rect::EVERYTHING, 1.5, &target(300, 200)),
            Some([0, 0, 300, 200])
        );
    }
}
