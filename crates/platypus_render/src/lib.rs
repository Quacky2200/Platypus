// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Platypus's 3D world pass (`world`) over web-ui's renderer, which this crate re-exports
//! whole (`Gpu`, surfaces, `UiRenderer`, `testing`, ...), so engine code keeps its paths.

pub use web_ui_render::*;

pub mod world;

pub use world::{
    ALL_LAYERS, Anisotropic, AutoExposure, BEAM_MESH, Bloom, CloudLayer, CookedTexture,
    DEFAULT_PARTICLE_TEXTURE, Denoising, ExposureReading, FillRule, FillVertex, Fog, FogMode,
    FogVolume, GizmoLine, Grading, HeightFog, Light, LightKind, MAX_CURVE_POINTS, MAX_FOG_VOLUMES,
    MISSING_TEXTURE, MaterialMaps, MeshData, MeshGlow, MeshInstance, Metering, Mirror, POST_PARAMS,
    PRIMITIVE_BASE, PRIMITIVES, PipelineAsset, PostEffect, PostParamType, PostShader, Projection,
    RENDER_TEXTURE_FORMAT, ReflectionProbe, RenderMode, ShadowQuality, Shadows, ShapeRun, Sky,
    Stroke, TexelFormat, TextureData, TextureFilter, TextureWrap, Tonemapping, VolumetricFog,
    WorldCamera, WorldQuad, WorldRenderer, WorldStats, WorldTexture, WorldView, in_mask, mip_chain,
    primitive, primitive_id, tessellate_fill, viewport_size,
};
