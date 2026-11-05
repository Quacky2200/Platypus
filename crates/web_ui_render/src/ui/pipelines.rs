// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The wgpu UI renderer's pipelines that draw into canvases. Their target format follows the
//! frame's blending (`canvas_format`), so there is one set per blending, made when first used.

use super::plan::Instance;

/// What the pipelines are made from: the two shader modules and the bind group layouts.
pub(super) struct Sources {
    pub quad_module: wgpu::ShaderModule,
    pub effects_module: wgpu::ShaderModule,
    pub quad_layout: wgpu::BindGroupLayout,
    pub blur_layout: wgpu::BindGroupLayout,
    pub composite_layout: wgpu::BindGroupLayout,
    pub blit_layout: wgpu::BindGroupLayout,
}

/// The pipelines drawing into canvases of one format.
pub(super) struct CanvasPipelines {
    pub quad: wgpu::RenderPipeline,
    pub image: wgpu::RenderPipeline,
    pub blur: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
    /// Composites with a blend mode: the shader mixes source and backdrop itself.
    pub composite_blend: wgpu::RenderPipeline,
}

/// How a blit converts the colours it copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Blit {
    /// As they are.
    Copy,
    /// Premultiplied sRGB values to linear ones (onto a target that encodes on write).
    Decode,
    /// Linear values to premultiplied sRGB ones (the world into an sRGB canvas).
    Encode,
}

impl Blit {
    pub fn entry(self) -> &'static str {
        match self {
            Blit::Copy => "fs_blit",
            Blit::Decode => "fs_blit_decode",
            Blit::Encode => "fs_blit_encode",
        }
    }
}

impl Sources {
    pub fn pipeline_layout(
        &self,
        device: &wgpu::Device,
        layouts: &[&wgpu::BindGroupLayout],
    ) -> wgpu::PipelineLayout {
        let layouts: Vec<Option<&wgpu::BindGroupLayout>> =
            layouts.iter().copied().map(Some).collect();
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &layouts,
            immediate_size: 0,
        })
    }

    pub fn canvas(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> CanvasPipelines {
        let instance_attributes = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4,
        ];
        let premultiplied = wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let quad_pass = |label, layout: &wgpu::PipelineLayout, entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &self.quad_module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attributes,
                    })],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.quad_module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(premultiplied.clone())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let quad = quad_pass(
            "ui quad",
            &self.pipeline_layout(device, &[&self.quad_layout]),
            "fs_main",
        );
        let image = quad_pass(
            "ui image",
            &self.pipeline_layout(device, &[&self.quad_layout, &self.blit_layout]),
            "fs_image",
        );
        let fullscreen =
            |label, layout: &wgpu::PipelineLayout, entry, target: wgpu::ColorTargetState| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: &self.effects_module,
                        entry_point: Some("vs_fullscreen"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.effects_module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(target)],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let replace = wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        };
        let composite_layout = self.pipeline_layout(device, &[&self.composite_layout]);
        CanvasPipelines {
            quad,
            image,
            blur: fullscreen(
                "ui blur",
                &self.pipeline_layout(device, &[&self.blur_layout]),
                "fs_blur",
                replace.clone(),
            ),
            composite: fullscreen(
                "ui composite",
                &composite_layout,
                "fs_composite",
                premultiplied.clone(),
            ),
            composite_blend: fullscreen(
                "ui composite blend",
                &composite_layout,
                "fs_composite_blend",
                replace,
            ),
        }
    }
}
