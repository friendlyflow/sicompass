//! Font rendering — FreeType glyph atlas + Vulkan text pipeline.
//!
//! Mirrors `text.c` / `text.h` from the C source.  Uses the raw `freetype`
//! crate (servo-style low-level bindings) exactly as the C code uses FreeType.

use crate::app_state::{SiError, MAX_FRAMES_IN_FLIGHT};
use crate::render;
use ash::vk;
use freetype::freetype as ft;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const FONT_ATLAS_SIZE: u32 = 1024;
pub const MAX_TEXT_VERTICES: usize = 1_048_576;
pub const FONT_SIZE_PT: f32 = 12.0;
pub const TEXT_PADDING: f32 = 4.0;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

pub struct GlyphInfo {
    pub size: [f32; 2],      // bitmap width, height
    pub bearing: [f32; 2],   // bitmap_left, bitmap_top
    pub advance: f32,         // horizontal advance (pixels at atlas size)
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

impl Default for GlyphInfo {
    fn default() -> Self {
        GlyphInfo { size: [0.0; 2], bearing: [0.0; 2], advance: 0.0, uv_min: [0.0; 2], uv_max: [0.0; 2] }
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

// ---------------------------------------------------------------------------
// FontRenderer
// ---------------------------------------------------------------------------

pub struct FontRenderer {
    // ---- FreeType state (raw pointers — single-threaded use only) ----------
    ft_library: ft::FT_Library,
    ft_face: ft::FT_Face,

    // ---- Glyph metrics and atlas -------------------------------------------
    pub glyphs: Vec<GlyphInfo>,   // indexed 0-255
    pub line_height: f32,
    pub ascender: f32,
    pub descender: f32,
    pub dpi: f32,

    // ---- Vulkan atlas resources ---------------------------------------------
    font_atlas_image: vk::Image,
    font_atlas_memory: vk::DeviceMemory,
    pub font_atlas_view: vk::ImageView,
    pub font_atlas_sampler: vk::Sampler,

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
}

// Safety: FontRenderer is only used from the main thread; raw FT pointers are opaque handles.
unsafe impl Send for FontRenderer {}

impl FontRenderer {
    /// Build the full font renderer: FreeType, glyph atlas, Vulkan pipeline.
    pub unsafe fn new(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        command_pool: vk::CommandPool,
        graphics_queue: vk::Queue,
        render_pass: vk::RenderPass,
    ) -> Result<Self, SiError> {
        // ----------------------------------------------------------------
        // 1. Init FreeType
        // ----------------------------------------------------------------
        let mut ft_library: ft::FT_Library = std::ptr::null_mut();
        if ft::FT_Init_FreeType(&mut ft_library) != 0 {
            return Err(SiError::Other("FreeType init failed".into()));
        }

        let font_path = std::ffi::CString::new("fonts/Consolas-Regular.ttf")
            .map_err(|e| SiError::Other(e.to_string()))?;
        let mut ft_face: ft::FT_Face = std::ptr::null_mut();
        if ft::FT_New_Face(ft_library, font_path.as_ptr(), 0, &mut ft_face) != 0 {
            ft::FT_Done_FreeType(ft_library);
            return Err(SiError::Other("Failed to load font 'fonts/Consolas-Regular.ttf'".into()));
        }

        // 64pt at 96 DPI (same as C code)
        let dpi = 96u32;
        if ft::FT_Set_Char_Size(ft_face, 0, 64 * 64, dpi, dpi) != 0 {
            return Err(SiError::Other("FT_Set_Char_Size failed".into()));
        }

        let size_metrics = (*(*ft_face).size).metrics;
        let ascender = size_metrics.ascender as f32 / 64.0;
        let descender = size_metrics.descender as f32 / 64.0;
        let line_height = ascender - descender;

        // ----------------------------------------------------------------
        // 2. Build glyph atlas (1024×1024 R8)
        // ----------------------------------------------------------------
        let atlas_sz = FONT_ATLAS_SIZE as usize;
        let mut atlas_data = vec![0u8; atlas_sz * atlas_sz];
        let mut glyphs: Vec<GlyphInfo> = (0..256).map(|_| GlyphInfo::default()).collect();

        let mut pen_x = 0i32;
        let mut pen_y = 0i32;
        let mut row_height = 0i32;

        for c in 32u32..256 {
            if ft::FT_Load_Char(ft_face, c as ft::FT_ULong, ft::FT_LOAD_RENDER as ft::FT_Int32) != 0 {
                continue;
            }
            let slot = (*ft_face).glyph;
            let bmp = &(*slot).bitmap;
            let bw = bmp.width as i32;
            let bh = bmp.rows as i32;

            if pen_x + bw >= FONT_ATLAS_SIZE as i32 {
                pen_x = 0;
                pen_y += row_height;
                row_height = 0;
            }

            if !bmp.buffer.is_null() && bw > 0 && bh > 0 {
                for row in 0..bh {
                    for col in 0..bw {
                        let x = pen_x + col;
                        let y = pen_y + row;
                        if x < atlas_sz as i32 && y < atlas_sz as i32 {
                            atlas_data[(y as usize) * atlas_sz + (x as usize)] =
                                *bmp.buffer.add((row * bw + col) as usize);
                        }
                    }
                }
            }

            let ci = c as usize;
            glyphs[ci].size = [bw as f32, bh as f32];
            glyphs[ci].bearing = [(*slot).bitmap_left as f32, (*slot).bitmap_top as f32];
            glyphs[ci].advance = ((*slot).advance.x >> 6) as f32;
            glyphs[ci].uv_min = [
                pen_x as f32 / FONT_ATLAS_SIZE as f32,
                pen_y as f32 / FONT_ATLAS_SIZE as f32,
            ];
            glyphs[ci].uv_max = [
                (pen_x + bw) as f32 / FONT_ATLAS_SIZE as f32,
                (pen_y + bh) as f32 / FONT_ATLAS_SIZE as f32,
            ];

            pen_x += bw + 1;
            if bh > row_height { row_height = bh; }
        }

        // ----------------------------------------------------------------
        // 3. Upload atlas to Vulkan (R8_UNORM, 1024×1024)
        // ----------------------------------------------------------------
        let atlas_bytes = (atlas_sz * atlas_sz) as vk::DeviceSize;
        let (staging_buf, staging_mem) = render::create_buffer(
            device, instance, physical_device,
            atlas_bytes,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        {
            let ptr = device.map_memory(staging_mem, 0, atlas_bytes, vk::MemoryMapFlags::empty())? as *mut u8;
            std::ptr::copy_nonoverlapping(atlas_data.as_ptr(), ptr, atlas_bytes as usize);
            device.unmap_memory(staging_mem);
        }

        let (font_atlas_image, font_atlas_memory) = render::create_image_helper(
            device, instance, physical_device,
            FONT_ATLAS_SIZE, FONT_ATLAS_SIZE,
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
            staging_buf, font_atlas_image, FONT_ATLAS_SIZE, FONT_ATLAS_SIZE,
        );
        render::transition_image_layout(
            device, command_pool, graphics_queue,
            font_atlas_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );

        device.destroy_buffer(staging_buf, None);
        device.free_memory(staging_mem, None);

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

        Ok(FontRenderer {
            ft_library,
            ft_face,
            glyphs,
            line_height,
            ascender,
            descender,
            dpi: dpi as f32,
            font_atlas_image,
            font_atlas_memory,
            font_atlas_view,
            font_atlas_sampler,
            vertex_buffer,
            vertex_buffer_memory,
            descriptor_set_layout,
            descriptor_pool,
            descriptor_sets,
            pipeline_layout,
            pipeline,
            vertices: Vec::with_capacity(8192),
        })
    }

    // ---- Frame helpers -----------------------------------------------------

    /// Reset the CPU-side vertex accumulator.
    pub fn begin_text_rendering(&mut self) {
        self.vertices.clear();
    }

    /// Append text quads to the CPU vertex buffer.
    pub fn prepare_text_for_rendering(&mut self, text: &str, x: f32, y: f32, scale: f32, color: u32) {
        let r = ((color >> 24) & 0xFF) as f32 / 255.0;
        let g = ((color >> 16) & 0xFF) as f32 / 255.0;
        let b = ((color >>  8) & 0xFF) as f32 / 255.0;
        let col = [r, g, b];

        let mut cx = x;
        for ch in text.chars() {
            let cp = ch as u32;
            if cp < 32 || cp >= 256 {
                // fallback advance (space width)
                cx += self.glyphs[b' ' as usize].advance * scale;
                continue;
            }
            let gi = &self.glyphs[cp as usize];

            let xpos = cx + gi.bearing[0] * scale;
            let ypos = y  - gi.bearing[1] * scale;
            let w = gi.size[0] * scale;
            let h = gi.size[1] * scale;
            let [u0, v0] = gi.uv_min;
            let [u1, v1] = gi.uv_max;

            if self.vertices.len() + 6 > MAX_TEXT_VERTICES { break; }

            // Two clockwise triangles (bottom-left origin, Y grows down)
            self.vertices.push(TextVertex { pos: [xpos,     ypos + h, 0.0], tex_coord: [u0, v1], color: col });
            self.vertices.push(TextVertex { pos: [xpos,     ypos,     0.0], tex_coord: [u0, v0], color: col });
            self.vertices.push(TextVertex { pos: [xpos + w, ypos,     0.0], tex_coord: [u1, v0], color: col });
            self.vertices.push(TextVertex { pos: [xpos,     ypos + h, 0.0], tex_coord: [u0, v1], color: col });
            self.vertices.push(TextVertex { pos: [xpos + w, ypos,     0.0], tex_coord: [u1, v0], color: col });
            self.vertices.push(TextVertex { pos: [xpos + w, ypos + h, 0.0], tex_coord: [u1, v1], color: col });

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
    ) {
        if self.vertices.is_empty() { return; }

        // Upload to GPU vertex buffer
        let upload_size = (std::mem::size_of::<TextVertex>() * self.vertices.len()) as vk::DeviceSize;
        let ptr = device
            .map_memory(self.vertex_buffer_memory, 0, upload_size, vk::MemoryMapFlags::empty())
            .unwrap() as *mut TextVertex;
        std::ptr::copy_nonoverlapping(self.vertices.as_ptr(), ptr, self.vertices.len());
        device.unmap_memory(self.vertex_buffer_memory);

        // Bind pipeline, push constants, descriptor set, vertex buffer, draw
        let screen = [extent.width as f32, extent.height as f32];
        device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
        device.cmd_push_constants(
            cb,
            self.pipeline_layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            std::slice::from_raw_parts(screen.as_ptr() as *const u8, 8),
        );
        device.cmd_bind_descriptor_sets(
            cb,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline_layout,
            0,
            &[self.descriptor_sets[frame % self.descriptor_sets.len()]],
            &[],
        );
        let bufs = [self.vertex_buffer];
        let offs = [0u64];
        device.cmd_bind_vertex_buffers(cb, 0, &bufs, &offs);
        device.cmd_draw(cb, self.vertices.len() as u32, 1, 0, 0);
    }

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
        self.glyphs[b'M' as usize].advance * scale
    }

    // ---- Cleanup -----------------------------------------------------------

    pub unsafe fn cleanup(&self, device: &ash::Device) {
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
        ft::FT_Done_Face(self.ft_face);
        ft::FT_Done_FreeType(self.ft_library);
    }
}
