//! Font rendering — FreeType glyph atlas + Vulkan text pipeline.
//!
//! Mirrors `text.c` / `text.h` from the C source.  Uses the raw `freetype`
//! crate (servo-style low-level bindings) exactly as the C code uses FreeType.

use crate::app_state::{SiError, MAX_FRAMES_IN_FLIGHT};
use crate::render;
use ash::vk;
use freetype::freetype as ft;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// FONT_ATLAS_SIZE is no longer a fixed constant — it scales with effective DPI in FontRenderer::new.
pub const MAX_TEXT_VERTICES: usize = 1_048_576;
pub const FONT_SIZE_PT: f32 = 12.0;
pub const TEXT_PADDING: f32 = 4.0;

/// Fallback font files tried, in order, after the primary `Consolas-Regular`
/// face for any codepoint the primary lacks. DejaVu covers Latin Extended,
/// Greek, Cyrillic, box-drawing, arrows and math symbols. (CJK is not covered
/// by these fonts — bundle e.g. Noto Sans Mono CJK and add it here to enable
/// it.)
const FALLBACK_FONTS: &[&str] = &[
    "fonts/DejaVuSansMono.ttf",
    "fonts/DejaVuSans.ttf",
];

/// Glyphs warmed into the atlas at startup so the common case (ASCII) is
/// resident on the very first frame and never needs a mid-frame re-upload.
const WARMUP_RANGE: std::ops::Range<u32> = 32..127;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct GlyphInfo {
    pub size: [f32; 2],      // bitmap width, height
    pub bearing: [f32; 2],   // bitmap_left, bitmap_top
    pub advance: f32,         // horizontal advance (pixels at atlas size)
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    // True when the glyph lives in the RGBA color atlas (emoji) rather than the
    // R8 coverage atlas. Color glyphs are drawn by a separate pipeline.
    pub is_color: bool,
}

impl Default for GlyphInfo {
    fn default() -> Self {
        GlyphInfo {
            size: [0.0; 2], bearing: [0.0; 2], advance: 0.0,
            uv_min: [0.0; 2], uv_max: [0.0; 2], is_color: false,
        }
    }
}

/// Per-vertex data for the text pipeline.
/// Layout must match `shaders/text.vert` attributes: pos(vec3), texCoord(vec2), color(vec3).
#[repr(C)]
pub struct TextVertex {
    pub pos: [f32; 3],
    pub tex_coord: [f32; 2],
    pub color: [f32; 3],
}

/// RGBA color-glyph (emoji) atlas + its dedicated pipeline. Present only when a
/// color font loaded successfully; absent in tests and on fonts without one.
/// Mirrors the monochrome atlas but stores premultiplied RGBA bitmaps and draws
/// them with the `text_color` pipeline (per-vertex color ignored).
struct ColorAtlas {
    // Color face, sized once to its fixed bitmap strike via `FT_Select_Size`.
    face: ft::FT_Face,
    // Maps the fixed strike's pixels into the monochrome raster space so emoji
    // size/advance compose with the same `* scale` math as text glyphs.
    strike_scale: f32,
    glyphs: RefCell<HashMap<u32, GlyphInfo>>,
    atlas_size: u32,
    atlas_data: RefCell<Vec<u8>>, // RGBA8, premultiplied alpha
    pen_x: Cell<i32>,
    pen_y: Cell<i32>,
    row_height: Cell<i32>,
    dirty: Cell<bool>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    sampler: vk::Sampler,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    descriptor_pool: vk::DescriptorPool,
    descriptor_sets: Vec<vk::DescriptorSet>,
    pipeline: vk::Pipeline,
    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,
}

// ---------------------------------------------------------------------------
// FontRenderer
// ---------------------------------------------------------------------------

pub struct FontRenderer {
    // ---- FreeType state (raw pointers — single-threaded use only) ----------
    ft_library: ft::FT_Library,
    // Primary face first, then the fallback faces (in `FALLBACK_FONTS` order).
    // A glyph is rasterized from the first face that contains its codepoint.
    faces: Vec<ft::FT_Face>,

    // ---- Glyph metrics and atlas -------------------------------------------
    // Codepoint -> glyph, populated lazily by `ensure_glyph`. Interior-mutable
    // so the measurement/layout API can stay `&self` while still rasterizing
    // on demand.
    glyphs: RefCell<HashMap<u32, GlyphInfo>>,
    // Codepoints no face provides (or that overflowed the atlas). Cached so a
    // repeated unsupported character isn't rescanned across every face/frame.
    missing: RefCell<std::collections::HashSet<u32>>,
    pub line_height: f32,
    pub ascender: f32,
    pub descender: f32,
    pub dpi: f32,

    // ---- Dynamic atlas (CPU-resident mirror + shelf packer) ----------------
    atlas_size: u32,
    atlas_data: RefCell<Vec<u8>>,
    pen_x: Cell<i32>,
    pen_y: Cell<i32>,
    row_height: Cell<i32>,
    // Set when a glyph is added; cleared by `flush_atlas` after re-upload.
    dirty: Cell<bool>,

    // ---- Vulkan atlas resources ---------------------------------------------
    font_atlas_image: vk::Image,
    font_atlas_memory: vk::DeviceMemory,
    pub font_atlas_view: vk::ImageView,
    pub font_atlas_sampler: vk::Sampler,
    // Persistent host-visible staging buffer reused by every `flush_atlas`.
    atlas_staging_buffer: vk::Buffer,
    atlas_staging_memory: vk::DeviceMemory,

    // ---- Vertex buffer (host-visible, host-coherent) -----------------------
    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,

    // ---- Descriptors -------------------------------------------------------
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_sets: Vec<vk::DescriptorSet>,

    // ---- Pipeline ----------------------------------------------------------
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,

    // ---- CPU-side vertex accumulator ---------------------------------------
    pub vertices: Vec<TextVertex>,

    // ---- Color-glyph (emoji) atlas + its own vertex accumulator ------------
    color: Option<ColorAtlas>,
    color_vertices: Vec<TextVertex>,
}

// Safety: FontRenderer is only used from the main thread; raw FT pointers are opaque handles.
unsafe impl Send for FontRenderer {}

/// Rasterize codepoint `cp` from the first face in `faces` that contains it,
/// pack it into the shelf described by (`pen_x`, `pen_y`, `row_height`), blit
/// its coverage bitmap into `atlas_data`, and return its metrics + UVs. Returns
/// `None` if no face provides the codepoint or the atlas is full. Shared by the
/// startup warmup loop and the on-demand `ensure_glyph` path.
unsafe fn rasterize_glyph(
    faces: &[ft::FT_Face],
    atlas_data: &mut [u8],
    atlas_size: u32,
    pen_x: &mut i32,
    pen_y: &mut i32,
    row_height: &mut i32,
    cp: u32,
) -> Option<GlyphInfo> { unsafe {
    let face = faces.iter().copied().find(|&f| {
        !f.is_null() && ft::FT_Get_Char_Index(f, cp as ft::FT_ULong) != 0
    })?;
    if ft::FT_Load_Char(face, cp as ft::FT_ULong, ft::FT_LOAD_RENDER as ft::FT_Int32) != 0 {
        return None;
    }
    let slot = (*face).glyph;
    let bmp = &(*slot).bitmap;
    let bw = bmp.width as i32;
    let bh = bmp.rows as i32;
    let asz = atlas_size as i32;

    // Advance to a new shelf row when the glyph would overrun the right edge.
    if *pen_x + bw >= asz {
        *pen_x = 0;
        *pen_y += *row_height;
        *row_height = 0;
    }
    if *pen_y + bh > asz {
        eprintln!(
            "text.rs: glyph atlas overflow for cp {cp:#x} at pen_y={}, atlas_size={atlas_size}",
            *pen_y
        );
        return None;
    }

    if !bmp.buffer.is_null() && bw > 0 && bh > 0 {
        let stride = atlas_size as usize;
        for row in 0..bh {
            for col in 0..bw {
                let x = *pen_x + col;
                let y = *pen_y + row;
                if x < asz && y < asz {
                    atlas_data[(y as usize) * stride + (x as usize)] =
                        *bmp.buffer.add((row * bw + col) as usize);
                }
            }
        }
    }

    let gi = GlyphInfo {
        size: [bw as f32, bh as f32],
        bearing: [(*slot).bitmap_left as f32, (*slot).bitmap_top as f32],
        advance: ((*slot).advance.x >> 6) as f32,
        uv_min: [*pen_x as f32 / atlas_size as f32, *pen_y as f32 / atlas_size as f32],
        uv_max: [
            (*pen_x + bw) as f32 / atlas_size as f32,
            (*pen_y + bh) as f32 / atlas_size as f32,
        ],
        is_color: false,
    };

    *pen_x += bw + 1;
    if bh > *row_height {
        *row_height = bh;
    }
    Some(gi)
}}

/// Rasterize color glyph `cp` (emoji) into the RGBA `atlas_data`, converting
/// FreeType's premultiplied BGRA to premultiplied RGBA. Metrics are scaled by
/// `strike_scale` into the monochrome raster space so emoji compose with the
/// same `* scale` math as text. Returns `None` if the face lacks the glyph,
/// has no bitmap, or the atlas is full.
unsafe fn rasterize_color_glyph(
    face: ft::FT_Face,
    strike_scale: f32,
    atlas_data: &mut [u8],
    atlas_size: u32,
    pen_x: &mut i32,
    pen_y: &mut i32,
    row_height: &mut i32,
    cp: u32,
) -> Option<GlyphInfo> { unsafe {
    if ft::FT_Get_Char_Index(face, cp as ft::FT_ULong) == 0 {
        return None;
    }
    let flags = (ft::FT_LOAD_RENDER | ft::FT_LOAD_COLOR) as ft::FT_Int32;
    if ft::FT_Load_Char(face, cp as ft::FT_ULong, flags) != 0 {
        return None;
    }
    let slot = (*face).glyph;
    let bmp = &(*slot).bitmap;
    let bw = bmp.width as i32;
    let bh = bmp.rows as i32;
    let asz = atlas_size as i32;
    if bw <= 0 || bh <= 0 || bmp.buffer.is_null() {
        return None;
    }

    if *pen_x + bw >= asz {
        *pen_x = 0;
        *pen_y += *row_height;
        *row_height = 0;
    }
    if *pen_y + bh > asz {
        eprintln!(
            "text.rs: color atlas overflow for cp {cp:#x} at pen_y={}, atlas_size={atlas_size}",
            *pen_y
        );
        return None;
    }

    let stride = atlas_size as usize * 4;
    let pitch = bmp.pitch; // bytes per source row; negative if bottom-up
    for row in 0..bh {
        // Map dest row to source row honoring pitch direction.
        let src_row = if pitch >= 0 { row } else { bh - 1 - row };
        let src_base = (src_row * pitch.abs()) as usize;
        for col in 0..bw {
            let src = bmp.buffer.add(src_base + (col * 4) as usize);
            let (b, g, r, a) = (*src, *src.add(1), *src.add(2), *src.add(3));
            let di = (*pen_y + row) as usize * stride + (*pen_x + col) as usize * 4;
            atlas_data[di] = r;
            atlas_data[di + 1] = g;
            atlas_data[di + 2] = b;
            atlas_data[di + 3] = a;
        }
    }

    let gi = GlyphInfo {
        size: [bw as f32 * strike_scale, bh as f32 * strike_scale],
        bearing: [
            (*slot).bitmap_left as f32 * strike_scale,
            (*slot).bitmap_top as f32 * strike_scale,
        ],
        advance: ((*slot).advance.x >> 6) as f32 * strike_scale,
        uv_min: [*pen_x as f32 / atlas_size as f32, *pen_y as f32 / atlas_size as f32],
        uv_max: [
            (*pen_x + bw) as f32 / atlas_size as f32,
            (*pen_y + bh) as f32 / atlas_size as f32,
        ],
        is_color: true,
    };

    *pen_x += bw + 1;
    if bh > *row_height {
        *row_height = bh;
    }
    Some(gi)
}}

impl ColorAtlas {
    /// Build the color atlas + pipeline from the bundled emoji font. Returns
    /// `None` (emoji disabled, text still works) if the font is missing or any
    /// resource fails. Reuses the text descriptor-set layout and pipeline
    /// layout; only the fragment shader and blend differ.
    unsafe fn new(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        command_pool: vk::CommandPool,
        graphics_queue: vk::Queue,
        render_pass: vk::RenderPass,
        descriptor_set_layout: vk::DescriptorSetLayout,
        pipeline_layout: vk::PipelineLayout,
        ft_library: ft::FT_Library,
        atlas_size: u32,
        raster_em_px: f32,
    ) -> Option<ColorAtlas> { unsafe {
        let path = std::ffi::CString::new("fonts/NotoColorEmoji.ttf").ok()?;
        let mut face: ft::FT_Face = std::ptr::null_mut();
        if ft::FT_New_Face(ft_library, path.as_ptr(), 0, &mut face) != 0 {
            eprintln!("text.rs: color emoji font 'fonts/NotoColorEmoji.ttf' not loaded; emoji disabled");
            return None;
        }
        // Bitmap-only color fonts have fixed strikes; pick the first.
        if (*face).num_fixed_sizes < 1 || ft::FT_Select_Size(face, 0) != 0 {
            eprintln!("text.rs: color emoji font has no usable strike; emoji disabled");
            ft::FT_Done_Face(face);
            return None;
        }
        let strike_ppem = (*(*face).size).metrics.y_ppem as f32;
        let strike_scale = if strike_ppem > 0.0 { raster_em_px / strike_ppem } else { 1.0 };

        let atlas_bytes = (atlas_size as usize * atlas_size as usize * 4) as vk::DeviceSize;
        let atlas_data = vec![0u8; atlas_bytes as usize];

        let (staging_buffer, staging_memory) = render::create_buffer(
            device, instance, physical_device, atlas_bytes,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        ).ok()?;
        {
            let ptr = device.map_memory(staging_memory, 0, atlas_bytes, vk::MemoryMapFlags::empty()).ok()? as *mut u8;
            std::ptr::copy_nonoverlapping(atlas_data.as_ptr(), ptr, atlas_bytes as usize);
            device.unmap_memory(staging_memory);
        }

        let (image, memory) = render::create_image_helper(
            device, instance, physical_device, atlas_size, atlas_size,
            vk::Format::R8G8B8A8_UNORM, vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        ).ok()?;
        render::transition_image_layout(device, command_pool, graphics_queue, image,
            vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
        render::copy_buffer_to_image(device, command_pool, graphics_queue, staging_buffer, image, atlas_size, atlas_size);
        render::transition_image_layout(device, command_pool, graphics_queue, image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0).level_count(1)
                    .base_array_layer(0).layer_count(1),
            );
        let view = device.create_image_view(&view_info, None).ok()?;

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            .unnormalized_coordinates(false)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
        let sampler = device.create_sampler(&sampler_info, None).ok()?;

        // Descriptor pool + sets bound to the color atlas.
        let pool_size = vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(MAX_FRAMES_IN_FLIGHT as u32);
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(std::slice::from_ref(&pool_size))
            .max_sets(MAX_FRAMES_IN_FLIGHT as u32);
        let descriptor_pool = device.create_descriptor_pool(&pool_info, None).ok()?;
        let layouts: Vec<vk::DescriptorSetLayout> = vec![descriptor_set_layout; MAX_FRAMES_IN_FLIGHT];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool)
            .set_layouts(&layouts);
        let descriptor_sets = device.allocate_descriptor_sets(&alloc_info).ok()?;
        let image_info = vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(view)
            .sampler(sampler);
        for &ds in &descriptor_sets {
            let write = vk::WriteDescriptorSet::default()
                .dst_set(ds).dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(std::slice::from_ref(&image_info));
            device.update_descriptor_sets(&[write], &[]);
        }

        // Color pipeline: same vertex format as text, premultiplied-alpha blend.
        let pipeline = build_color_pipeline(device, render_pass, pipeline_layout).ok()?;

        // Per-frame color vertex buffer (same capacity as the text buffer).
        let vb_size = (std::mem::size_of::<TextVertex>() * MAX_TEXT_VERTICES) as vk::DeviceSize;
        let (vertex_buffer, vertex_buffer_memory) = render::create_buffer(
            device, instance, physical_device, vb_size,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        ).ok()?;

        Some(ColorAtlas {
            face,
            strike_scale,
            glyphs: RefCell::new(HashMap::new()),
            atlas_size,
            atlas_data: RefCell::new(atlas_data),
            pen_x: Cell::new(0),
            pen_y: Cell::new(0),
            row_height: Cell::new(0),
            dirty: Cell::new(false),
            image,
            memory,
            view,
            sampler,
            staging_buffer,
            staging_memory,
            descriptor_pool,
            descriptor_sets,
            pipeline,
            vertex_buffer,
            vertex_buffer_memory,
        })
    }}

    /// Return `cp`'s color glyph, rasterizing it on first use.
    fn ensure(&self, cp: u32) -> Option<GlyphInfo> {
        if let Some(g) = self.glyphs.borrow().get(&cp).copied() {
            return Some(g);
        }
        let mut px = self.pen_x.get();
        let mut py = self.pen_y.get();
        let mut rh = self.row_height.get();
        let result = {
            let mut data = self.atlas_data.borrow_mut();
            unsafe {
                rasterize_color_glyph(self.face, self.strike_scale, &mut data, self.atlas_size, &mut px, &mut py, &mut rh, cp)
            }
        };
        self.pen_x.set(px);
        self.pen_y.set(py);
        self.row_height.set(rh);
        if let Some(g) = result {
            self.dirty.set(true);
            self.glyphs.borrow_mut().insert(cp, g);
        }
        result
    }

    /// Re-upload the RGBA atlas if dirty (see `FontRenderer::flush_atlas`).
    unsafe fn flush(&self, device: &ash::Device, command_pool: vk::CommandPool, queue: vk::Queue) { unsafe {
        if !self.dirty.get() {
            return;
        }
        let atlas_bytes = (self.atlas_size as usize * self.atlas_size as usize * 4) as vk::DeviceSize;
        {
            let data = self.atlas_data.borrow();
            let ptr = device.map_memory(self.staging_memory, 0, atlas_bytes, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, atlas_bytes as usize);
            device.unmap_memory(self.staging_memory);
        }
        render::transition_image_layout(device, command_pool, queue, self.image,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
        render::copy_buffer_to_image(device, command_pool, queue, self.staging_buffer, self.image, self.atlas_size, self.atlas_size);
        render::transition_image_layout(device, command_pool, queue, self.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        self.dirty.set(false);
    }}

    unsafe fn destroy(&self, device: &ash::Device) { unsafe {
        device.destroy_pipeline(self.pipeline, None);
        device.destroy_descriptor_pool(self.descriptor_pool, None);
        device.destroy_buffer(self.vertex_buffer, None);
        device.free_memory(self.vertex_buffer_memory, None);
        device.destroy_sampler(self.sampler, None);
        device.destroy_image_view(self.view, None);
        device.destroy_image(self.image, None);
        device.free_memory(self.memory, None);
        device.destroy_buffer(self.staging_buffer, None);
        device.free_memory(self.staging_memory, None);
        ft::FT_Done_Face(self.face);
    }}
}

/// Build the color-glyph pipeline: identical to the text pipeline except for
/// the fragment shader and a premultiplied-alpha blend (emoji texels are
/// premultiplied).
unsafe fn build_color_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
) -> Result<vk::Pipeline, SiError> { unsafe {
    let vert_code = std::fs::read("shaders/text_vert.spv")
        .map_err(|e| SiError::Other(format!("text_vert.spv: {e}")))?;
    let frag_code = std::fs::read("shaders/text_color_frag.spv")
        .map_err(|e| SiError::Other(format!("text_color_frag.spv: {e}")))?;
    let vert_module = render::create_shader_module(device, &vert_code)?;
    let frag_module = render::create_shader_module(device, &frag_code)?;

    let entry = std::ffi::CString::new("main").unwrap();
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX).module(vert_module).name(&entry),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT).module(frag_module).name(&entry),
    ];

    let stride = std::mem::size_of::<TextVertex>() as u32;
    let binding_desc = vk::VertexInputBindingDescription::default()
        .binding(0).stride(stride).input_rate(vk::VertexInputRate::VERTEX);
    let attr_descs = [
        vk::VertexInputAttributeDescription::default().location(0).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(0),
        vk::VertexInputAttributeDescription::default().location(1).binding(0).format(vk::Format::R32G32_SFLOAT).offset(12),
        vk::VertexInputAttributeDescription::default().location(2).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(20),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(std::slice::from_ref(&binding_desc))
        .vertex_attribute_descriptions(&attr_descs);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
    let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL).line_width(1.0)
        .cull_mode(vk::CullModeFlags::NONE).front_face(vk::FrontFace::COUNTER_CLOCKWISE);
    let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(false).depth_write_enable(false).depth_compare_op(vk::CompareOp::ALWAYS);

    // Premultiplied-alpha blend: src factor ONE (texels already premultiplied).
    let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::R | vk::ColorComponentFlags::G | vk::ColorComponentFlags::B | vk::ColorComponentFlags::A)
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD);
    let blend_state = vk::PipelineColorBlendStateCreateInfo::default()
        .attachments(std::slice::from_ref(&blend_attachment));

    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterizer)
        .multisample_state(&multisampling)
        .depth_stencil_state(&depth_stencil)
        .color_blend_state(&blend_state)
        .dynamic_state(&dynamic_state)
        .layout(pipeline_layout)
        .render_pass(render_pass)
        .subpass(0);
    let pipeline = device
        .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
        .map_err(|(_, e)| SiError::Vulkan(e))?[0];

    device.destroy_shader_module(vert_module, None);
    device.destroy_shader_module(frag_module, None);
    Ok(pipeline)
}}

impl FontRenderer {
    /// Build the full font renderer: FreeType, glyph atlas, Vulkan pipeline.
    pub unsafe fn new(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        command_pool: vk::CommandPool,
        graphics_queue: vk::Queue,
        render_pass: vk::RenderPass,
        dpi: u32,
    ) -> Result<Self, SiError> { unsafe {
        // ----------------------------------------------------------------
        // 1. Init FreeType + load the primary face and the fallback chain
        // ----------------------------------------------------------------
        let mut ft_library: ft::FT_Library = std::ptr::null_mut();
        if ft::FT_Init_FreeType(&mut ft_library) != 0 {
            return Err(SiError::Other("FreeType init failed".into()));
        }

        let mut faces: Vec<ft::FT_Face> = Vec::new();

        // Primary face — its absence is fatal (everything falls back to it).
        let primary_path = std::ffi::CString::new("fonts/Consolas-Regular.ttf")
            .map_err(|e| SiError::Other(e.to_string()))?;
        let mut primary: ft::FT_Face = std::ptr::null_mut();
        if ft::FT_New_Face(ft_library, primary_path.as_ptr(), 0, &mut primary) != 0 {
            ft::FT_Done_FreeType(ft_library);
            return Err(SiError::Other("Failed to load font 'fonts/Consolas-Regular.ttf'".into()));
        }
        faces.push(primary);

        // Fallback faces — optional: a missing fallback only narrows coverage.
        for path in FALLBACK_FONTS {
            let cpath = std::ffi::CString::new(*path)
                .map_err(|e| SiError::Other(e.to_string()))?;
            let mut face: ft::FT_Face = std::ptr::null_mut();
            if ft::FT_New_Face(ft_library, cpath.as_ptr(), 0, &mut face) == 0 {
                faces.push(face);
            } else {
                eprintln!("text.rs: optional fallback font '{path}' could not be loaded");
            }
        }

        // 64pt rasterised at the supplied DPI (typically 96 × content_scale × font_scale)
        for &face in &faces {
            if ft::FT_Set_Char_Size(face, 0, 64 * 64, dpi, dpi) != 0 {
                return Err(SiError::Other("FT_Set_Char_Size failed".into()));
            }
        }

        // Scale the atlas with DPI so glyphs always fit.  At 96 DPI the atlas
        // is 1024² (baseline, unchanged).  Each doubling of DPI doubles both
        // glyph dimensions, so we need 4× the area — i.e. 2× the linear size.
        // Use round() not ceil() so small DPI bumps (e.g. 97 at 100% scale)
        // don't prematurely jump to 2048², quadrupling startup cost.
        let atlas_ratio = ((dpi as f32) / 96.0).round().max(1.0) as u32;
        let font_atlas_size: u32 = (1024 * atlas_ratio).min(8192);

        // Line metrics come from the primary face.
        let size_metrics = (*(*faces[0]).size).metrics;
        let ascender = size_metrics.ascender as f32 / 64.0;
        let descender = size_metrics.descender as f32 / 64.0;
        let line_height = ascender - descender;

        // ----------------------------------------------------------------
        // 2. Build the (initially ASCII-warmed) glyph atlas. Glyphs outside
        //    the warmup range are rasterized lazily by `ensure_glyph` and the
        //    dirty atlas re-uploaded by `flush_atlas`.
        // ----------------------------------------------------------------
        let atlas_sz = font_atlas_size as usize;
        let mut atlas_data = vec![0u8; atlas_sz * atlas_sz];
        let mut glyphs: HashMap<u32, GlyphInfo> = HashMap::new();

        let mut pen_x = 0i32;
        let mut pen_y = 0i32;
        let mut row_height = 0i32;

        for c in WARMUP_RANGE {
            if let Some(g) = rasterize_glyph(
                &faces, &mut atlas_data, font_atlas_size,
                &mut pen_x, &mut pen_y, &mut row_height, c,
            ) {
                glyphs.insert(c, g);
            }
        }

        // ----------------------------------------------------------------
        // 3. Upload atlas to Vulkan (R8_UNORM). The staging buffer is kept
        //    around so `flush_atlas` can re-upload newly rasterized glyphs.
        // ----------------------------------------------------------------
        let atlas_bytes = (atlas_sz * atlas_sz) as vk::DeviceSize;
        let (atlas_staging_buffer, atlas_staging_memory) = render::create_buffer(
            device, instance, physical_device,
            atlas_bytes,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        {
            let ptr = device.map_memory(atlas_staging_memory, 0, atlas_bytes, vk::MemoryMapFlags::empty())? as *mut u8;
            std::ptr::copy_nonoverlapping(atlas_data.as_ptr(), ptr, atlas_bytes as usize);
            device.unmap_memory(atlas_staging_memory);
        }

        let (font_atlas_image, font_atlas_memory) = render::create_image_helper(
            device, instance, physical_device,
            font_atlas_size, font_atlas_size,
            vk::Format::R8_UNORM,
            vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        render::transition_image_layout(
            device, command_pool, graphics_queue,
            font_atlas_image,
            vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
        render::copy_buffer_to_image(
            device, command_pool, graphics_queue,
            atlas_staging_buffer, font_atlas_image, font_atlas_size, font_atlas_size,
        );
        render::transition_image_layout(
            device, command_pool, graphics_queue,
            font_atlas_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );

        // ----------------------------------------------------------------
        // 4. Image view + sampler
        // ----------------------------------------------------------------
        let view_info = vk::ImageViewCreateInfo::default()
            .image(font_atlas_image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::R8_UNORM)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0).level_count(1)
                    .base_array_layer(0).layer_count(1),
            );
        let font_atlas_view = device.create_image_view(&view_info, None)?;

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .anisotropy_enable(false)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
        let font_atlas_sampler = device.create_sampler(&sampler_info, None)?;

        // ----------------------------------------------------------------
        // 5. Vertex buffer (host-visible, host-coherent)
        // ----------------------------------------------------------------
        let vb_size = (std::mem::size_of::<TextVertex>() * MAX_TEXT_VERTICES) as vk::DeviceSize;
        let (vertex_buffer, vertex_buffer_memory) = render::create_buffer(
            device, instance, physical_device,
            vb_size,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        // ----------------------------------------------------------------
        // 6. Descriptor set layout + pool + sets
        // ----------------------------------------------------------------
        let sampler_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT);
        let ds_layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(std::slice::from_ref(&sampler_binding));
        let descriptor_set_layout = device.create_descriptor_set_layout(&ds_layout_info, None)?;

        let pool_size = vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(MAX_FRAMES_IN_FLIGHT as u32);
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(std::slice::from_ref(&pool_size))
            .max_sets(MAX_FRAMES_IN_FLIGHT as u32);
        let descriptor_pool = device.create_descriptor_pool(&pool_info, None)?;

        let layouts: Vec<vk::DescriptorSetLayout> = vec![descriptor_set_layout; MAX_FRAMES_IN_FLIGHT];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool)
            .set_layouts(&layouts);
        let descriptor_sets = device.allocate_descriptor_sets(&alloc_info)?;

        let image_info = vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(font_atlas_view)
            .sampler(font_atlas_sampler);
        for &ds in &descriptor_sets {
            let write = vk::WriteDescriptorSet::default()
                .dst_set(ds)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(std::slice::from_ref(&image_info));
            device.update_descriptor_sets(&[write], &[]);
        }

        // ----------------------------------------------------------------
        // 7. Graphics pipeline
        // ----------------------------------------------------------------
        let vert_code = std::fs::read("shaders/text_vert.spv")
            .map_err(|e| SiError::Other(format!("text_vert.spv: {e}")))?;
        let frag_code = std::fs::read("shaders/text_frag.spv")
            .map_err(|e| SiError::Other(format!("text_frag.spv: {e}")))?;
        let vert_module = render::create_shader_module(device, &vert_code)?;
        let frag_module = render::create_shader_module(device, &frag_code)?;

        let entry = std::ffi::CString::new("main").unwrap();
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(&entry),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(&entry),
        ];

        let stride = std::mem::size_of::<TextVertex>() as u32;
        let binding_desc = vk::VertexInputBindingDescription::default()
            .binding(0).stride(stride).input_rate(vk::VertexInputRate::VERTEX);
        // Offsets: pos@0 (12B), texCoord@12 (8B), color@20 (12B)
        let attr_descs = [
            vk::VertexInputAttributeDescription::default()
                .location(0).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default()
                .location(1).binding(0).format(vk::Format::R32G32_SFLOAT).offset(12),
            vk::VertexInputAttributeDescription::default()
                .location(2).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(20),
        ];

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(std::slice::from_ref(&binding_desc))
            .vertex_attribute_descriptions(&attr_descs);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1).scissor_count(1);

        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE);

        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(false)
            .depth_write_enable(false)
            .depth_compare_op(vk::CompareOp::ALWAYS);

        let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(
                vk::ColorComponentFlags::R | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B | vk::ColorComponentFlags::A,
            )
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
            .alpha_blend_op(vk::BlendOp::ADD);

        let blend_state = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&blend_attachment));

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state = vk::PipelineDynamicStateCreateInfo::default()
            .dynamic_states(&dynamic_states);

        let push_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(0)
            .size(std::mem::size_of::<[f32; 2]>() as u32);

        let pl_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(std::slice::from_ref(&descriptor_set_layout))
            .push_constant_ranges(std::slice::from_ref(&push_range));
        let pipeline_layout = device.create_pipeline_layout(&pl_info, None)?;

        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&blend_state)
            .dynamic_state(&dynamic_state)
            .layout(pipeline_layout)
            .render_pass(render_pass)
            .subpass(0);

        let pipeline = device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .map_err(|(_, e)| SiError::Vulkan(e))?[0];

        device.destroy_shader_module(vert_module, None);
        device.destroy_shader_module(frag_module, None);

        // Optional color-glyph atlas (emoji). Failure disables emoji, not text.
        let raster_em_px = 64.0 * dpi as f32 / 72.0;
        let color = ColorAtlas::new(
            device, instance, physical_device, command_pool, graphics_queue,
            render_pass, descriptor_set_layout, pipeline_layout, ft_library,
            font_atlas_size, raster_em_px,
        );

        Ok(FontRenderer {
            ft_library,
            faces,
            glyphs: RefCell::new(glyphs),
            missing: RefCell::new(std::collections::HashSet::new()),
            line_height,
            ascender,
            descender,
            dpi: dpi as f32,
            atlas_size: font_atlas_size,
            atlas_data: RefCell::new(atlas_data),
            pen_x: Cell::new(pen_x),
            pen_y: Cell::new(pen_y),
            row_height: Cell::new(row_height),
            dirty: Cell::new(false),
            font_atlas_image,
            font_atlas_memory,
            font_atlas_view,
            font_atlas_sampler,
            atlas_staging_buffer,
            atlas_staging_memory,
            vertex_buffer,
            vertex_buffer_memory,
            descriptor_set_layout,
            descriptor_pool,
            descriptor_sets,
            pipeline_layout,
            pipeline,
            vertices: Vec::with_capacity(8192),
            color,
            color_vertices: Vec::new(),
        })
    }}

    /// Free all Vulkan and FreeType resources.
    /// The caller must ensure the device is idle before calling this
    /// (e.g. `device.device_wait_idle().unwrap()`).
    pub unsafe fn destroy(&self, device: &ash::Device) { unsafe {
        device.destroy_pipeline(self.pipeline, None);
        device.destroy_pipeline_layout(self.pipeline_layout, None);
        device.destroy_descriptor_pool(self.descriptor_pool, None);
        device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        device.destroy_buffer(self.vertex_buffer, None);
        device.free_memory(self.vertex_buffer_memory, None);
        device.destroy_sampler(self.font_atlas_sampler, None);
        device.destroy_image_view(self.font_atlas_view, None);
        device.destroy_image(self.font_atlas_image, None);
        device.free_memory(self.font_atlas_memory, None);
        device.destroy_buffer(self.atlas_staging_buffer, None);
        device.free_memory(self.atlas_staging_memory, None);
        if let Some(ca) = &self.color {
            ca.destroy(device);
        }
        for &face in &self.faces {
            ft::FT_Done_Face(face);
        }
        ft::FT_Done_FreeType(self.ft_library);
    }}

    // ---- Dynamic glyph atlas ----------------------------------------------

    /// Return `cp`'s glyph, rasterizing and packing it into the atlas on first
    /// use. Returns `None` when no loaded face contains the codepoint or the
    /// atlas is full; the caller then advances like a space. Cheap and `&self`
    /// so the measurement/layout API can call it freely.
    fn ensure_glyph(&self, cp: u32) -> Option<GlyphInfo> {
        if cp < 32 {
            return None;
        }
        if let Some(g) = self.glyphs.borrow().get(&cp).copied() {
            return Some(g);
        }
        if let Some(ca) = &self.color {
            if let Some(g) = ca.glyphs.borrow().get(&cp).copied() {
                return Some(g);
            }
        }
        if self.missing.borrow().contains(&cp) {
            return None;
        }

        // Monochrome faces first (text, including symbols both fonts share),
        // then the color atlas (pictographic emoji absent from the text fonts).
        let mut px = self.pen_x.get();
        let mut py = self.pen_y.get();
        let mut rh = self.row_height.get();
        let result = {
            let mut data = self.atlas_data.borrow_mut();
            unsafe {
                rasterize_glyph(&self.faces, &mut data, self.atlas_size, &mut px, &mut py, &mut rh, cp)
            }
        };
        self.pen_x.set(px);
        self.pen_y.set(py);
        self.row_height.set(rh);

        if let Some(g) = result {
            self.dirty.set(true);
            self.glyphs.borrow_mut().insert(cp, g);
            return Some(g);
        }
        if let Some(ca) = &self.color {
            if let Some(g) = ca.ensure(cp) {
                return Some(g);
            }
        }
        self.missing.borrow_mut().insert(cp);
        None
    }

    /// Horizontal advance of `ch` in atlas pixels, ensuring the glyph exists.
    /// Unknown codepoints fall back to the space advance so layout progresses.
    fn advance(&self, ch: char) -> f32 {
        if let Some(g) = self.ensure_glyph(ch as u32) {
            if g.advance > 0.0 {
                return g.advance;
            }
        }
        self.ensure_glyph(' ' as u32).map(|g| g.advance).unwrap_or(0.0)
    }

    /// Re-upload the CPU atlas to the GPU image if glyphs were added since the
    /// last flush. Must run outside a render pass (it transitions image layout)
    /// and after all `prepare_*`/measurement calls for the frame.
    pub unsafe fn flush_atlas(
        &self,
        device: &ash::Device,
        command_pool: vk::CommandPool,
        queue: vk::Queue,
    ) { unsafe {
        if self.dirty.get() {
            let atlas_bytes = (self.atlas_size as usize * self.atlas_size as usize) as vk::DeviceSize;
            {
                let data = self.atlas_data.borrow();
                let ptr = device
                    .map_memory(self.atlas_staging_memory, 0, atlas_bytes, vk::MemoryMapFlags::empty())
                    .unwrap() as *mut u8;
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, atlas_bytes as usize);
                device.unmap_memory(self.atlas_staging_memory);
            }
            render::transition_image_layout(
                device, command_pool, queue, self.font_atlas_image,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            render::copy_buffer_to_image(
                device, command_pool, queue,
                self.atlas_staging_buffer, self.font_atlas_image, self.atlas_size, self.atlas_size,
            );
            render::transition_image_layout(
                device, command_pool, queue, self.font_atlas_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
            self.dirty.set(false);
        }

        if let Some(ca) = &self.color {
            ca.flush(device, command_pool, queue);
        }
    }}

    // ---- Frame helpers -----------------------------------------------------

    /// Reset the CPU-side vertex accumulators.
    pub fn begin_text_rendering(&mut self) {
        self.vertices.clear();
        self.color_vertices.clear();
    }

    /// Append text quads to the CPU vertex buffer.
    pub fn prepare_text_for_rendering(&mut self, text: &str, x: f32, y: f32, scale: f32, color: u32) {
        let r = ((color >> 24) & 0xFF) as f32 / 255.0;
        let g = ((color >> 16) & 0xFF) as f32 / 255.0;
        let b = ((color >>  8) & 0xFF) as f32 / 255.0;
        let col = [r, g, b];

        let mut cx = x;
        let space_adv = self.ensure_glyph(' ' as u32).map(|g| g.advance).unwrap_or(0.0);
        for ch in text.chars() {
            // Rasterize on demand; an unsupported codepoint advances like a space.
            let gi = match self.ensure_glyph(ch as u32) {
                Some(g) => g,
                None => {
                    cx += space_adv * scale;
                    continue;
                }
            };

            // Glyphs with no bitmap (e.g. space) still advance but emit no quad.
            if gi.size[0] > 0.0 && gi.size[1] > 0.0 {
                let xpos = cx + gi.bearing[0] * scale;
                let ypos = y - gi.bearing[1] * scale;
                let w = gi.size[0] * scale;
                let h = gi.size[1] * scale;
                let [u0, v0] = gi.uv_min;
                let [u1, v1] = gi.uv_max;

                // Color glyphs (emoji) go to the separate RGBA pipeline.
                let buf = if gi.is_color { &mut self.color_vertices } else { &mut self.vertices };
                if buf.len() + 6 <= MAX_TEXT_VERTICES {
                    // Two clockwise triangles (bottom-left origin, Y grows down)
                    buf.push(TextVertex { pos: [xpos,     ypos + h, 0.0], tex_coord: [u0, v1], color: col });
                    buf.push(TextVertex { pos: [xpos,     ypos,     0.0], tex_coord: [u0, v0], color: col });
                    buf.push(TextVertex { pos: [xpos + w, ypos,     0.0], tex_coord: [u1, v0], color: col });
                    buf.push(TextVertex { pos: [xpos,     ypos + h, 0.0], tex_coord: [u0, v1], color: col });
                    buf.push(TextVertex { pos: [xpos + w, ypos,     0.0], tex_coord: [u1, v0], color: col });
                    buf.push(TextVertex { pos: [xpos + w, ypos + h, 0.0], tex_coord: [u1, v1], color: col });
                }
            }

            cx += gi.advance * scale;
        }
    }

    /// Upload vertex data and issue draw command.
    pub unsafe fn draw_text(
        &self,
        device: &ash::Device,
        cb: vk::CommandBuffer,
        frame: usize,
        extent: vk::Extent2D,
    ) { unsafe {
        let screen = [extent.width as f32, extent.height as f32];

        // Monochrome text pass.
        if !self.vertices.is_empty() {
            let upload_size = (std::mem::size_of::<TextVertex>() * self.vertices.len()) as vk::DeviceSize;
            let ptr = device
                .map_memory(self.vertex_buffer_memory, 0, upload_size, vk::MemoryMapFlags::empty())
                .unwrap() as *mut TextVertex;
            std::ptr::copy_nonoverlapping(self.vertices.as_ptr(), ptr, self.vertices.len());
            device.unmap_memory(self.vertex_buffer_memory);

            device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_push_constants(
                cb, self.pipeline_layout, vk::ShaderStageFlags::VERTEX, 0,
                std::slice::from_raw_parts(screen.as_ptr() as *const u8, 8),
            );
            device.cmd_bind_descriptor_sets(
                cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0,
                &[self.descriptor_sets[frame % self.descriptor_sets.len()]], &[],
            );
            device.cmd_bind_vertex_buffers(cb, 0, &[self.vertex_buffer], &[0]);
            device.cmd_draw(cb, self.vertices.len() as u32, 1, 0, 0);
        }

        // Color-glyph (emoji) pass — shares the pipeline layout, uses the RGBA
        // pipeline + descriptor set bound to the color atlas.
        if let Some(ca) = &self.color {
            if !self.color_vertices.is_empty() {
                let upload_size = (std::mem::size_of::<TextVertex>() * self.color_vertices.len()) as vk::DeviceSize;
                let ptr = device
                    .map_memory(ca.vertex_buffer_memory, 0, upload_size, vk::MemoryMapFlags::empty())
                    .unwrap() as *mut TextVertex;
                std::ptr::copy_nonoverlapping(self.color_vertices.as_ptr(), ptr, self.color_vertices.len());
                device.unmap_memory(ca.vertex_buffer_memory);

                device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, ca.pipeline);
                device.cmd_push_constants(
                    cb, self.pipeline_layout, vk::ShaderStageFlags::VERTEX, 0,
                    std::slice::from_raw_parts(screen.as_ptr() as *const u8, 8),
                );
                device.cmd_bind_descriptor_sets(
                    cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0,
                    &[ca.descriptor_sets[frame % ca.descriptor_sets.len()]], &[],
                );
                device.cmd_bind_vertex_buffers(cb, 0, &[ca.vertex_buffer], &[0]);
                device.cmd_draw(cb, self.color_vertices.len() as u32, 1, 0, 0);
            }
        }
    }}

    // ---- Font metric helpers (mirrors C text.c) ----------------------------

    /// Convert a desired point size to a scale factor for glyph coordinates.
    pub fn get_text_scale(&self, desired_pt: f32) -> f32 {
        let desired_px = desired_pt * self.dpi / 72.0;
        desired_px / self.line_height
    }

    /// Line height in pixels at the given scale plus top/bottom padding.
    pub fn get_line_height(&self, scale: f32, padding: f32) -> f32 {
        self.line_height * scale + padding * 2.0
    }

    /// Width of the 'M' character in pixels at the given scale (em width).
    pub fn get_width_em(&self, scale: f32) -> f32 {
        self.ensure_glyph('M' as u32).map(|g| g.advance).unwrap_or(0.0) * scale
    }

    /// Pixel width of `text` at `scale` (sum of glyph advances).
    pub fn measure_text_width(&self, text: &str, scale: f32) -> f32 {
        text.chars().map(|ch| self.advance(ch) * scale).sum()
    }

    /// Number of lines required to render `text` with word-wrapping at `max_width` pixels.
    pub fn count_wrapped_lines(&self, text: &str, scale: f32, max_width: f32) -> usize {
        self.compute_wrap_lines(text, scale, max_width).len().max(1)
    }

    /// Render `text` with word-wrapping. All lines start at `x`; the first line
    /// baseline is `y`, subsequent lines advance by `line_height`. Returns line count.
    pub fn prepare_text_wrapped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        max_width: f32,
        line_height: f32,
        color: u32,
    ) -> usize {
        let lines = self.compute_wrap_lines(text, scale, max_width);
        let count = lines.len().max(1);
        for (n, line) in lines.iter().enumerate() {
            let line_y = y + n as f32 * line_height;
            self.prepare_text_for_rendering(line, x, line_y, scale, color);
        }
        count
    }

    /// Word-wrap `text`, returning each line with its starting byte offset in the original text.
    pub fn wrap_lines_with_offsets(&self, text: &str, scale: f32, max_width: f32) -> Vec<(String, usize)> {
        self.compute_wrap_lines_hanging(text, scale, max_width, max_width)
    }

    /// Word-wrap `text` with the first line wrapped at `first_width` and every
    /// subsequent line at `rest_width` (a reverse hanging indent: a wide
    /// content column whose first line is shortened by a preceding
    /// breadcrumb/prefix). Returns each line with its starting byte offset.
    pub fn wrap_lines_with_offsets_hanging(
        &self,
        text: &str,
        scale: f32,
        first_width: f32,
        rest_width: f32,
    ) -> Vec<(String, usize)> {
        self.compute_wrap_lines_hanging(text, scale, first_width, rest_width)
    }

    /// Word-wrap `text` into lines with their starting byte offsets. The first
    /// line is wrapped at `first_width`, every subsequent line at `rest_width`
    /// (pass the same value for both for uniform wrapping). Explicit `\n` also
    /// break lines.
    fn compute_wrap_lines_hanging(
        &self,
        text: &str,
        scale: f32,
        first_width: f32,
        rest_width: f32,
    ) -> Vec<(String, usize)> {
        if text.is_empty() {
            return vec![(String::new(), 0)];
        }

        // Advance of the char starting at byte `i`, decoded as one codepoint
        // (`i` is always on a char boundary). Unknown codepoints advance like
        // a space so wrapping still progresses.
        let adv = |i: usize| -> f32 {
            let ch = text[i..].chars().next().unwrap_or(' ');
            self.advance(ch) * scale
        };

        let bytes = text.as_bytes();
        let n = bytes.len();
        let mut lines: Vec<(String, usize)> = Vec::new();
        let mut line_start = 0usize;
        let mut line_width = 0.0f32;
        let mut last_space: Option<usize> = None;
        let mut last_fit = 0usize;
        let mut i = 0usize;

        let char_len = |b: u8| -> usize {
            if b < 0x80 { 1 } else if b < 0xE0 { 2 } else if b < 0xF0 { 3 } else { 4 }
        };

        while i < n {
            let b = bytes[i];
            let clen = char_len(b);
            // The first line is narrowed by the breadcrumb/prefix; the rest
            // use the full content width.
            let max_width = if lines.is_empty() { first_width } else { rest_width };

            if b == b'\n' {
                lines.push((text[line_start..i].to_owned(), line_start));
                line_start = i + 1;
                line_width = 0.0;
                last_space = None;
                last_fit = line_start;
                i = line_start;
                continue;
            }

            let next_width = line_width + adv(i);

            if next_width > max_width && i > line_start {
                let break_end = if let Some(sp) = last_space {
                    sp
                } else {
                    last_fit.max(line_start + char_len(bytes[line_start]))
                };
                lines.push((text[line_start..break_end].to_owned(), line_start));
                line_start = break_end;
                if line_start < n && bytes[line_start] == b' ' {
                    line_start += 1;
                }
                line_width = 0.0;
                last_space = None;
                last_fit = line_start;
                i = line_start;
                continue;
            }

            if b == b' ' {
                last_space = Some(i);
            }
            last_fit = i + clen;
            line_width = next_width;
            i += clen;
        }

        lines.push((text[line_start..].to_owned(), line_start));
        lines
    }

    /// Split `text` into wrapped line strings given `max_width` pixels per line.
    fn compute_wrap_lines(&self, text: &str, scale: f32, max_width: f32) -> Vec<String> {
        if text.is_empty() {
            return vec![String::new()];
        }

        // Advance of the char starting at byte `i`, decoded as one codepoint
        // (`i` is always on a char boundary). Unknown codepoints advance like
        // a space so wrapping still progresses.
        let adv = |i: usize| -> f32 {
            let ch = text[i..].chars().next().unwrap_or(' ');
            self.advance(ch) * scale
        };

        let bytes = text.as_bytes();
        let n = bytes.len();
        let mut lines: Vec<String> = Vec::new();
        let mut line_start = 0usize;
        let mut line_width = 0.0f32;
        let mut last_space: Option<usize> = None;
        let mut last_fit = 0usize;
        let mut i = 0usize;

        // Returns the byte length of the UTF-8 char starting at `pos`.
        let char_len = |b: u8| -> usize {
            if b < 0x80 { 1 } else if b < 0xE0 { 2 } else if b < 0xF0 { 3 } else { 4 }
        };

        while i < n {
            let b = bytes[i];
            let clen = char_len(b);

            if b == b'\n' {
                lines.push(text[line_start..i].to_owned());
                line_start = i + 1;
                line_width = 0.0;
                last_space = None;
                last_fit = line_start;
                i = line_start;
                continue;
            }

            let next_width = line_width + adv(i);

            if next_width > max_width && i > line_start {
                let break_end = if let Some(sp) = last_space {
                    sp
                } else {
                    // last_fit is always on a char boundary; fall back to
                    // the end of the first char at line_start so we always
                    // make progress without splitting a multi-byte sequence.
                    last_fit.max(line_start + char_len(bytes[line_start]))
                };
                lines.push(text[line_start..break_end].to_owned());
                line_start = break_end;
                if line_start < n && bytes[line_start] == b' ' {
                    line_start += 1;
                }
                line_width = 0.0;
                last_space = None;
                last_fit = line_start;
                i = line_start;
                continue;
            }

            if b == b' ' {
                last_space = Some(i);
            }
            last_fit = i + clen;
            line_width = next_width;
            i += clen;
        }

        lines.push(text[line_start..].to_owned());
        lines
    }

    // ---- Cleanup -----------------------------------------------------------

    pub unsafe fn cleanup(&self, device: &ash::Device) { unsafe {
        device.destroy_pipeline(self.pipeline, None);
        device.destroy_pipeline_layout(self.pipeline_layout, None);
        device.destroy_descriptor_pool(self.descriptor_pool, None);
        device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        device.destroy_buffer(self.vertex_buffer, None);
        device.free_memory(self.vertex_buffer_memory, None);
        device.destroy_sampler(self.font_atlas_sampler, None);
        device.destroy_image_view(self.font_atlas_view, None);
        device.destroy_image(self.font_atlas_image, None);
        device.free_memory(self.font_atlas_memory, None);
        device.destroy_buffer(self.atlas_staging_buffer, None);
        device.free_memory(self.atlas_staging_memory, None);
        if let Some(ca) = &self.color {
            ca.destroy(device);
        }
        for &face in &self.faces {
            ft::FT_Done_Face(face);
        }
        ft::FT_Done_FreeType(self.ft_library);
    }}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal FontRenderer with zeroed Vulkan/FT handles for math-only
    /// tests. With no faces loaded, `ensure_glyph` never rasterizes — it only
    /// reads back the pre-seeded `glyphs` map, so these tests stay Vulkan-free.
    fn fr_from_glyphs(dpi: f32, line_height: f32, glyphs: HashMap<u32, GlyphInfo>) -> FontRenderer {
        FontRenderer {
            ft_library: std::ptr::null_mut(),
            faces: Vec::new(),
            glyphs: RefCell::new(glyphs),
            missing: RefCell::new(std::collections::HashSet::new()),
            line_height,
            ascender: line_height * 0.8,
            descender: -(line_height * 0.2),
            dpi,
            atlas_size: 0,
            atlas_data: RefCell::new(Vec::new()),
            pen_x: Cell::new(0),
            pen_y: Cell::new(0),
            row_height: Cell::new(0),
            dirty: Cell::new(false),
            font_atlas_image: vk::Image::null(),
            font_atlas_memory: vk::DeviceMemory::null(),
            font_atlas_view: vk::ImageView::null(),
            font_atlas_sampler: vk::Sampler::null(),
            atlas_staging_buffer: vk::Buffer::null(),
            atlas_staging_memory: vk::DeviceMemory::null(),
            vertex_buffer: vk::Buffer::null(),
            vertex_buffer_memory: vk::DeviceMemory::null(),
            descriptor_set_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            descriptor_sets: Vec::new(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            vertices: Vec::new(),
            color: None,
            color_vertices: Vec::new(),
        }
    }

    fn make_fr(dpi: f32, line_height: f32, m_advance: f32) -> FontRenderer {
        let mut glyphs = HashMap::new();
        glyphs.insert('M' as u32, GlyphInfo { advance: m_advance, ..GlyphInfo::default() });
        fr_from_glyphs(dpi, line_height, glyphs)
    }

    // ---- get_text_scale ---

    #[test]
    fn text_scale_12pt_96dpi() {
        let fr = make_fr(96.0, 20.0, 10.0);
        // 12pt at 96dpi = 16px; scale = 16/20 = 0.8
        let s = fr.get_text_scale(12.0);
        assert!((s - 0.8).abs() < 1e-3, "expected 0.8, got {s}");
    }

    #[test]
    fn text_scale_24pt_96dpi() {
        let fr = make_fr(96.0, 20.0, 10.0);
        // 24pt at 96dpi = 32px; scale = 32/20 = 1.6
        let s = fr.get_text_scale(24.0);
        assert!((s - 1.6).abs() < 1e-3, "expected 1.6, got {s}");
    }

    #[test]
    fn text_scale_12pt_144dpi() {
        let fr = make_fr(144.0, 20.0, 10.0);
        // 12pt at 144dpi = 24px; scale = 24/20 = 1.2
        let s = fr.get_text_scale(12.0);
        assert!((s - 1.2).abs() < 1e-3, "expected 1.2, got {s}");
    }

    #[test]
    fn text_scale_proportional_to_pt_size() {
        let fr = make_fr(96.0, 20.0, 10.0);
        let s12 = fr.get_text_scale(12.0);
        let s24 = fr.get_text_scale(24.0);
        assert!((s24 - s12 * 2.0).abs() < 1e-3);
    }

    #[test]
    fn text_scale_proportional_to_dpi() {
        let fr96 = make_fr(96.0, 20.0, 10.0);
        let fr192 = make_fr(192.0, 20.0, 10.0);
        let s96 = fr96.get_text_scale(12.0);
        let s192 = fr192.get_text_scale(12.0);
        assert!((s192 - s96 * 2.0).abs() < 1e-3);
    }

    #[test]
    fn text_scale_inversely_proportional_to_line_height() {
        let fr20 = make_fr(96.0, 20.0, 10.0);
        let fr40 = make_fr(96.0, 40.0, 10.0);
        let s20 = fr20.get_text_scale(12.0);
        let s40 = fr40.get_text_scale(12.0);
        assert!((s20 - s40 * 2.0).abs() < 1e-3);
    }

    // ---- get_width_em ---

    #[test]
    fn width_em_basic() {
        let fr = make_fr(96.0, 20.0, 10.0);
        assert!((fr.get_width_em(1.0) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn width_em_scaled() {
        let fr = make_fr(96.0, 20.0, 10.0);
        assert!((fr.get_width_em(2.5) - 25.0).abs() < 1e-3);
    }

    #[test]
    fn width_em_zero_scale() {
        let fr = make_fr(96.0, 20.0, 10.0);
        assert!((fr.get_width_em(0.0) - 0.0).abs() < 1e-3);
    }

    // ---- get_line_height ---

    #[test]
    fn line_height_no_padding() {
        let fr = make_fr(96.0, 20.0, 10.0);
        assert!((fr.get_line_height(1.0, 0.0) - 20.0).abs() < 1e-3);
    }

    #[test]
    fn line_height_with_padding() {
        let fr = make_fr(96.0, 20.0, 10.0);
        // 20 * 1.0 + 4 * 2 = 28
        assert!((fr.get_line_height(1.0, 4.0) - 28.0).abs() < 1e-3);
    }

    #[test]
    fn line_height_scaled() {
        let fr = make_fr(96.0, 20.0, 10.0);
        // 20 * 2.0 = 40
        assert!((fr.get_line_height(2.0, 0.0) - 40.0).abs() < 1e-3);
    }

    #[test]
    fn line_height_scaled_with_padding() {
        let fr = make_fr(96.0, 20.0, 10.0);
        // 20 * 0.5 + 3 * 2 = 16
        assert!((fr.get_line_height(0.5, 3.0) - 16.0).abs() < 1e-3);
    }

    #[test]
    fn line_height_different_font() {
        let fr = make_fr(96.0, 30.0, 10.0);
        // 30 * 1.0 + 2 * 2 = 34
        assert!((fr.get_line_height(1.0, 2.0) - 34.0).abs() < 1e-3);
    }

    // ---- measure_text_width ---

    fn make_fr_uniform(advance: f32) -> FontRenderer {
        let mut glyphs = HashMap::new();
        for cp in 32u32..256 {
            glyphs.insert(cp, GlyphInfo { advance, ..GlyphInfo::default() });
        }
        fr_from_glyphs(96.0, 20.0, glyphs)
    }

    #[test]
    fn measure_empty() {
        let fr = make_fr_uniform(10.0);
        assert!((fr.measure_text_width("", 1.0) - 0.0).abs() < 1e-3);
    }

    #[test]
    fn measure_three_chars() {
        let fr = make_fr_uniform(10.0);
        assert!((fr.measure_text_width("abc", 1.0) - 30.0).abs() < 1e-3);
    }

    // ---- count_wrapped_lines ---

    #[test]
    fn wrap_single_line_fits() {
        let fr = make_fr_uniform(10.0);
        // "hello" = 5 chars * 10px = 50px, max 100px -> 1 line
        assert_eq!(fr.count_wrapped_lines("hello", 1.0, 100.0), 1);
    }

    #[test]
    fn wrap_two_words_wrap() {
        let fr = make_fr_uniform(10.0);
        // "hello world" = 11 chars * 10px = 110px, max 100px -> break at space -> 2 lines
        assert_eq!(fr.count_wrapped_lines("hello world", 1.0, 100.0), 2);
    }

    #[test]
    fn wrap_empty_text() {
        let fr = make_fr_uniform(10.0);
        assert_eq!(fr.count_wrapped_lines("", 1.0, 100.0), 1);
    }

    #[test]
    fn wrap_force_break_no_space() {
        let fr = make_fr_uniform(10.0);
        // "abcdefghijk" = 11 chars * 10px = 110px, max 100px, no space -> force break at 10 -> 2 lines
        assert_eq!(fr.count_wrapped_lines("abcdefghijk", 1.0, 100.0), 2);
    }

    #[test]
    fn wrap_three_lines() {
        let fr = make_fr_uniform(10.0);
        // 15 chars: "aaa bbb ccc ddd" = break at each space when max=50 (5 chars)
        // "aaa b" = 50px (fits exactly at 50), next...
        // Actually "aaa " = 40px, "aaa b" = 50px exactly -> does NOT exceed 50, so continues
        // "aaa bb" = 60px > 50 -> break at last space (after "aaa") -> "aaa", then "bbb ccc ddd"
        // "bbb c" = 50px, "bbb cc" = 60 > 50 -> break at space -> "bbb", then "ccc ddd"
        // "ccc d" = 50, "ccc dd" = 60 > 50 -> break at space -> "ccc", then "ddd"
        // Result: ["aaa", "bbb", "ccc", "ddd"] = 4 lines
        assert_eq!(fr.count_wrapped_lines("aaa bbb ccc ddd", 1.0, 50.0), 4);
    }

    // ---- wrap_lines_with_offsets ---

    #[test]
    fn wrap_offsets_empty() {
        let fr = make_fr_uniform(10.0);
        let result = fr.wrap_lines_with_offsets("", 1.0, 100.0);
        assert_eq!(result, vec![("".to_string(), 0)]);
    }

    #[test]
    fn wrap_offsets_single_line() {
        let fr = make_fr_uniform(10.0);
        // "hello" = 50px < 100px, fits on one line
        let result = fr.wrap_lines_with_offsets("hello", 1.0, 100.0);
        assert_eq!(result, vec![("hello".to_string(), 0)]);
    }

    #[test]
    fn wrap_offsets_two_words() {
        let fr = make_fr_uniform(10.0);
        // "hello world" = 110px > 100px → breaks at space after "hello"
        // second line "world" starts at byte offset 6 ("hello " = 6 bytes)
        let result = fr.wrap_lines_with_offsets("hello world", 1.0, 100.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("hello".to_string(), 0));
        assert_eq!(result[1], ("world".to_string(), 6));
    }

    #[test]
    fn wrap_offsets_force_break() {
        let fr = make_fr_uniform(10.0);
        // "abcdefghijk" = 11 chars, no spaces, max=100px → force-break after 10 chars
        let result = fr.wrap_lines_with_offsets("abcdefghijk", 1.0, 100.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "abcdefghij");
        assert_eq!(result[0].1, 0);
        assert_eq!(result[1].0, "k");
        assert_eq!(result[1].1, 10);
    }

    #[test]
    fn wrap_offsets_newline_split() {
        let fr = make_fr_uniform(10.0);
        // Explicit newline → two lines, "def" starts at byte 4 ("abc\n")
        let result = fr.wrap_lines_with_offsets("abc\ndef", 1.0, 100.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("abc".to_string(), 0));
        assert_eq!(result[1], ("def".to_string(), 4));
    }

    // ---- wrap_lines_with_offsets_hanging ---

    #[test]
    fn wrap_hanging_first_line_narrower() {
        let fr = make_fr_uniform(10.0);
        // first_width = 25px fits "aa" (20px) but not "aa " (30px); the wider
        // rest_width = 100px fits the remaining "bb cc dd" (80px) on one line.
        let result = fr.wrap_lines_with_offsets_hanging("aa bb cc dd", 1.0, 25.0, 100.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("aa".to_string(), 0));
        assert_eq!(result[1], ("bb cc dd".to_string(), 3));
    }

    #[test]
    fn wrap_hanging_equal_widths_matches_uniform() {
        let fr = make_fr_uniform(10.0);
        let hanging = fr.wrap_lines_with_offsets_hanging("hello world", 1.0, 100.0, 100.0);
        let uniform = fr.wrap_lines_with_offsets("hello world", 1.0, 100.0);
        assert_eq!(hanging, uniform);
    }

    // ---- dynamic atlas + fallback (real FreeType, no Vulkan) ---

    /// Load the primary + fallback faces from the repo's bundled fonts using an
    /// absolute path (tests run with CWD = crate dir, not the repo root).
    unsafe fn load_test_faces() -> (ft::FT_Library, Vec<ft::FT_Face>) { unsafe {
        let mut lib: ft::FT_Library = std::ptr::null_mut();
        assert_eq!(ft::FT_Init_FreeType(&mut lib), 0, "FreeType init failed");
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let mut faces = Vec::new();
        for name in ["Consolas-Regular.ttf", "DejaVuSansMono.ttf", "DejaVuSans.ttf"] {
            let p = std::ffi::CString::new(format!("{root}/fonts/{name}")).unwrap();
            let mut face: ft::FT_Face = std::ptr::null_mut();
            if ft::FT_New_Face(lib, p.as_ptr(), 0, &mut face) == 0 {
                assert_eq!(ft::FT_Set_Char_Size(face, 0, 64 * 64, 96, 96), 0);
                faces.push(face);
            }
        }
        (lib, faces)
    }}

    #[test]
    fn dynamic_atlas_rasterizes_ascii_extended_and_fallback() {
        unsafe {
            let (lib, faces) = load_test_faces();
            assert!(!faces.is_empty(), "bundled test fonts not found under fonts/");

            let atlas_size = 1024u32;
            let mut atlas = vec![0u8; (atlas_size * atlas_size) as usize];
            let (mut px, mut py, mut rh) = (0i32, 0i32, 0i32);

            // ASCII 'A' from the primary face: rasterized with a real bitmap.
            let a = rasterize_glyph(&faces, &mut atlas, atlas_size, &mut px, &mut py, &mut rh, 'A' as u32);
            assert!(a.is_some_and(|g| g.size[0] > 0.0 && g.size[1] > 0.0), "ascii 'A' not rasterized");

            // Box-drawing vertical bar and Greek alpha: codepoints outside
            // Latin-1, covered via the DejaVu fallback faces.
            let bar = rasterize_glyph(&faces, &mut atlas, atlas_size, &mut px, &mut py, &mut rh, 0x2502);
            assert!(bar.is_some_and(|g| g.advance > 0.0), "box-drawing glyph missing");
            let alpha = rasterize_glyph(&faces, &mut atlas, atlas_size, &mut px, &mut py, &mut rh, 0x03B1);
            assert!(alpha.is_some_and(|g| g.size[0] > 0.0 && g.size[1] > 0.0), "Greek alpha missing");

            // The shelf packer advances: each glyph lands at distinct UVs, and
            // coverage was actually blitted into the atlas.
            assert_ne!(a.unwrap().uv_min, alpha.unwrap().uv_min, "glyphs overlap in atlas");
            assert!(atlas.iter().any(|&b| b != 0), "no coverage written to atlas");

            for f in faces {
                ft::FT_Done_Face(f);
            }
            ft::FT_Done_FreeType(lib);
        }
    }

    #[test]
    fn color_atlas_rasterizes_emoji_to_rgba() {
        unsafe {
            let mut lib: ft::FT_Library = std::ptr::null_mut();
            assert_eq!(ft::FT_Init_FreeType(&mut lib), 0);
            let path = std::ffi::CString::new(concat!(
                env!("CARGO_MANIFEST_DIR"), "/../../fonts/NotoColorEmoji.ttf"
            )).unwrap();
            let mut face: ft::FT_Face = std::ptr::null_mut();
            if ft::FT_New_Face(lib, path.as_ptr(), 0, &mut face) != 0 {
                ft::FT_Done_FreeType(lib);
                panic!("bundled NotoColorEmoji.ttf not found");
            }
            assert!((*face).num_fixed_sizes >= 1, "emoji font has no bitmap strike");
            assert_eq!(ft::FT_Select_Size(face, 0), 0);
            let strike_ppem = (*(*face).size).metrics.y_ppem as f32;
            // Map the strike to a ~16px raster em (12pt @ 96dpi ≈ 16px).
            let strike_scale = (64.0 * 96.0 / 72.0) / strike_ppem;

            let atlas_size = 512u32;
            let mut atlas = vec![0u8; (atlas_size * atlas_size * 4) as usize];
            let (mut px, mut py, mut rh) = (0i32, 0i32, 0i32);

            // 😀 U+1F600 grinning face — a pictographic emoji.
            let g = rasterize_color_glyph(face, strike_scale, &mut atlas, atlas_size, &mut px, &mut py, &mut rh, 0x1F600);
            assert!(g.is_some_and(|g| g.is_color && g.size[0] > 0.0 && g.size[1] > 0.0),
                "emoji not rasterized as a color glyph");
            // The bitmap was scaled down toward the text size, not left at the
            // full strike resolution.
            assert!(g.unwrap().size[1] < strike_ppem, "emoji not scaled into text space");
            // Color (non-alpha) channels were written, not just coverage.
            assert!(atlas.chunks(4).any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0),
                "no RGB color written to the emoji atlas");

            ft::FT_Done_Face(face);
            ft::FT_Done_FreeType(lib);
        }
    }
}
