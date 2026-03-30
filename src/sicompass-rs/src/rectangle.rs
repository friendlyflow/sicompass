//! Rectangle rendering — SDF rounded-rect Vulkan pipeline.
//!
//! Mirrors `rectangle.c` / `rectangle.h` from the C source.

use crate::app_state::SiError;
use crate::render;
use ash::vk;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_RECT_VERTICES: usize = 200 * 6; // 200 rectangles × 6 vertices

// ---------------------------------------------------------------------------
// Vertex layout (must match shaders/rectangle.vert attributes)
// ---------------------------------------------------------------------------

/// Per-vertex data for the rectangle pipeline.
/// Attribute layout: pos(vec2)@0, color(vec4)@8, cornerRadius(vec2)@24,
///                   rectSize(vec2)@32, rectOrigin(vec2)@40
#[repr(C)]
pub struct RectVertex {
    pub pos: [f32; 2],           // screen-space pixel position
    pub color: [f32; 4],         // RGBA
    pub corner_radius: [f32; 2], // x=radius, y=unused
    pub rect_size: [f32; 2],     // width, height
    pub rect_origin: [f32; 2],   // top-left corner (minX, minY)
}

// ---------------------------------------------------------------------------
// RectangleRenderer
// ---------------------------------------------------------------------------

pub struct RectangleRenderer {
    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    // CPU-side vertex accumulator
    pub vertices: Vec<RectVertex>,
}

impl RectangleRenderer {
    pub unsafe fn new(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        render_pass: vk::RenderPass,
    ) -> Result<Self, SiError> {
        // ---- Vertex buffer (host-visible, host-coherent) -------------------
        let vb_size = (std::mem::size_of::<RectVertex>() * MAX_RECT_VERTICES) as vk::DeviceSize;
        let (vertex_buffer, vertex_buffer_memory) = render::create_buffer(
            device, instance, physical_device,
            vb_size,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        // ---- Pipeline ------------------------------------------------------
        let vert_code = std::fs::read("shaders/rectangle_vert.spv")
            .map_err(|e| SiError::Other(format!("rectangle_vert.spv: {e}")))?;
        let frag_code = std::fs::read("shaders/rectangle_frag.spv")
            .map_err(|e| SiError::Other(format!("rectangle_frag.spv: {e}")))?;
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

        let stride = std::mem::size_of::<RectVertex>() as u32;
        let binding_desc = vk::VertexInputBindingDescription::default()
            .binding(0).stride(stride).input_rate(vk::VertexInputRate::VERTEX);
        // Offsets: pos@0(8B), color@8(16B), cornerRadius@24(8B), rectSize@32(8B), rectOrigin@40(8B)
        let attr_descs = [
            vk::VertexInputAttributeDescription::default()
                .location(0).binding(0).format(vk::Format::R32G32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default()
                .location(1).binding(0).format(vk::Format::R32G32B32A32_SFLOAT).offset(8),
            vk::VertexInputAttributeDescription::default()
                .location(2).binding(0).format(vk::Format::R32G32_SFLOAT).offset(24),
            vk::VertexInputAttributeDescription::default()
                .location(3).binding(0).format(vk::Format::R32G32_SFLOAT).offset(32),
            vk::VertexInputAttributeDescription::default()
                .location(4).binding(0).format(vk::Format::R32G32_SFLOAT).offset(40),
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

        // Push constant: screenWidth, screenHeight (vec2 in vertex shader)
        let push_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(0)
            .size(std::mem::size_of::<[f32; 2]>() as u32);

        let pl_info = vk::PipelineLayoutCreateInfo::default()
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

        Ok(RectangleRenderer {
            vertex_buffer,
            vertex_buffer_memory,
            pipeline_layout,
            pipeline,
            vertices: Vec::with_capacity(128),
        })
    }

    // ---- Frame helpers -----------------------------------------------------

    /// Reset vertex accumulator for new frame.
    pub fn begin_rect_rendering(&mut self) {
        self.vertices.clear();
    }

    /// Append a filled rectangle.
    pub fn prepare_rectangle(
        &mut self,
        x: f32, y: f32, width: f32, height: f32,
        color: u32, corner_radius: f32,
    ) {
        if self.vertices.len() >= MAX_RECT_VERTICES { return; }

        let r = ((color >> 24) & 0xFF) as f32 / 255.0;
        let g = ((color >> 16) & 0xFF) as f32 / 255.0;
        let b = ((color >>  8) & 0xFF) as f32 / 255.0;
        let a = ( color        & 0xFF) as f32 / 255.0;
        let col = [r, g, b, a];

        let max_r = (width.min(height) * 0.5).min(corner_radius);
        let cr = [max_r, 0.0];
        let rs = [width, height];
        let ro = [x, y];

        let (x0, y0, x1, y1) = (x, y, x + width, y + height);

        // Triangle 1
        self.vertices.push(RectVertex { pos: [x0, y0], color: col, corner_radius: cr, rect_size: rs, rect_origin: ro });
        self.vertices.push(RectVertex { pos: [x1, y0], color: col, corner_radius: cr, rect_size: rs, rect_origin: ro });
        self.vertices.push(RectVertex { pos: [x1, y1], color: col, corner_radius: cr, rect_size: rs, rect_origin: ro });
        // Triangle 2
        self.vertices.push(RectVertex { pos: [x0, y0], color: col, corner_radius: cr, rect_size: rs, rect_origin: ro });
        self.vertices.push(RectVertex { pos: [x1, y1], color: col, corner_radius: cr, rect_size: rs, rect_origin: ro });
        self.vertices.push(RectVertex { pos: [x0, y1], color: col, corner_radius: cr, rect_size: rs, rect_origin: ro });
    }

    /// Upload vertices and issue draw command.
    pub unsafe fn draw_rectangles(
        &self,
        device: &ash::Device,
        cb: vk::CommandBuffer,
        extent: vk::Extent2D,
    ) {
        if self.vertices.is_empty() { return; }

        let upload_size = (std::mem::size_of::<RectVertex>() * self.vertices.len()) as vk::DeviceSize;
        let ptr = device
            .map_memory(self.vertex_buffer_memory, 0, upload_size, vk::MemoryMapFlags::empty())
            .unwrap() as *mut RectVertex;
        std::ptr::copy_nonoverlapping(self.vertices.as_ptr(), ptr, self.vertices.len());
        device.unmap_memory(self.vertex_buffer_memory);

        let screen = [extent.width as f32, extent.height as f32];
        device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
        device.cmd_push_constants(
            cb,
            self.pipeline_layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            std::slice::from_raw_parts(screen.as_ptr() as *const u8, 8),
        );
        let bufs = [self.vertex_buffer];
        let offs = [0u64];
        device.cmd_bind_vertex_buffers(cb, 0, &bufs, &offs);
        device.cmd_draw(cb, self.vertices.len() as u32, 1, 0, 0);
    }

    // ---- Cleanup -----------------------------------------------------------

    pub unsafe fn cleanup(&self, device: &ash::Device) {
        device.destroy_pipeline(self.pipeline, None);
        device.destroy_pipeline_layout(self.pipeline_layout, None);
        device.destroy_buffer(self.vertex_buffer, None);
        device.free_memory(self.vertex_buffer_memory, None);
    }
}
