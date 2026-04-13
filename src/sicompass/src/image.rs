//! Image rendering — Vulkan pipeline for textured quads.
//!
//! Mirrors `image.c` / `image.h` from the C source.
//!
//! Each call to [`ImageRenderer::prepare_image`] looks up (or loads) a texture
//! by file path and records a draw quad.  Up to [`MAX_CACHED_IMAGES`] textures
//! are kept in an LRU cache; the least-recently-used entry is evicted when the
//! cache is full.  All quads are batched and drawn in a single `draw_images`
//! call at the end of each frame.

use crate::app_state::SiError;
use crate::render;
use ash::vk;
use std::ptr;

// Use the `image` crate under an alias to avoid shadowing this module's name.
use ::image as img_crate;
use img_crate::GenericImageView as _;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_CACHED_IMAGES: usize = 16;
const VERTS_PER_QUAD: usize = 6;
const MAX_IMAGE_VERTICES: usize = MAX_CACHED_IMAGES * VERTS_PER_QUAD;

// ---------------------------------------------------------------------------
// Vertex layout (must match shaders/image_vert.spv)
// ---------------------------------------------------------------------------

/// Per-vertex data: screen-space position + UV texture coordinate.
#[repr(C)]
struct ImageVertex {
    pos: [f32; 2],
    tex_coord: [f32; 2],
}

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

struct CachedTexture {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    sampler: vk::Sampler,
    path: String,
    width: u32,
    height: u32,
    last_used_frame: u64,
}

// ---------------------------------------------------------------------------
// Per-frame draw record
// ---------------------------------------------------------------------------

struct ImageDraw {
    slot: usize,
    vertex_offset: u32,
}

// ---------------------------------------------------------------------------
// ImageRenderer
// ---------------------------------------------------------------------------

pub struct ImageRenderer {
    // Vulkan context (cloned handles — ash wraps in Arc internally)
    device: ash::Device,
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    queue: vk::Queue,

    // GPU resources
    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layout: vk::DescriptorSetLayout,
    /// Pre-allocated descriptor sets — one per cache slot.
    descriptor_sets: Vec<vk::DescriptorSet>,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,

    // Texture cache: MAX_CACHED_IMAGES slots, None = empty.
    cache: Vec<Option<CachedTexture>>,

    // Per-frame CPU accumulators
    draws: Vec<ImageDraw>,
    vertices: Vec<ImageVertex>,
    current_frame: u64,
}

impl ImageRenderer {
    pub unsafe fn new(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        command_pool: vk::CommandPool,
        queue: vk::Queue,
        render_pass: vk::RenderPass,
    ) -> Result<Self, SiError> {
        // ---- Vertex buffer (host-visible, host-coherent) ----------------------
        let vb_size = (std::mem::size_of::<ImageVertex>() * MAX_IMAGE_VERTICES) as vk::DeviceSize;
        let (vertex_buffer, vertex_buffer_memory) = render::create_buffer(
            device, instance, physical_device,
            vb_size,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        // ---- Descriptor set layout -------------------------------------------
        let sampler_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT);
        let dsl_info = vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(std::slice::from_ref(&sampler_binding));
        let descriptor_set_layout = device.create_descriptor_set_layout(&dsl_info, None)?;

        // ---- Descriptor pool (one set per cache slot) -------------------------
        let pool_size = vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(MAX_CACHED_IMAGES as u32);
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(MAX_CACHED_IMAGES as u32)
            .pool_sizes(std::slice::from_ref(&pool_size));
        let descriptor_pool = device.create_descriptor_pool(&pool_info, None)?;

        // Allocate all sets upfront
        let layouts = vec![descriptor_set_layout; MAX_CACHED_IMAGES];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool)
            .set_layouts(&layouts);
        let descriptor_sets = device.allocate_descriptor_sets(&alloc_info)?;

        // ---- Pipeline --------------------------------------------------------
        let vert_code = std::fs::read("shaders/image_vert.spv")
            .map_err(|e| SiError::Other(format!("image_vert.spv: {e}")))?;
        let frag_code = std::fs::read("shaders/image_frag.spv")
            .map_err(|e| SiError::Other(format!("image_frag.spv: {e}")))?;
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

        let stride = std::mem::size_of::<ImageVertex>() as u32;
        let binding_desc = vk::VertexInputBindingDescription::default()
            .binding(0).stride(stride).input_rate(vk::VertexInputRate::VERTEX);
        // pos@0(8B), texCoord@8(8B)
        let attr_descs = [
            vk::VertexInputAttributeDescription::default()
                .location(0).binding(0).format(vk::Format::R32G32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default()
                .location(1).binding(0).format(vk::Format::R32G32_SFLOAT).offset(8),
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

        let set_layouts = [descriptor_set_layout];
        let pl_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
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

        Ok(ImageRenderer {
            device: device.clone(),
            instance: instance.clone(),
            physical_device,
            command_pool,
            queue,
            vertex_buffer,
            vertex_buffer_memory,
            descriptor_pool,
            descriptor_set_layout,
            descriptor_sets,
            pipeline_layout,
            pipeline,
            cache: (0..MAX_CACHED_IMAGES).map(|_| None).collect(),
            draws: Vec::new(),
            vertices: Vec::with_capacity(MAX_IMAGE_VERTICES),
            current_frame: 0,
        })
    }

    // ---- Frame helpers -------------------------------------------------------

    /// Reset per-frame draw list. Call once at the start of each frame.
    pub fn begin_image_rendering(&mut self) {
        self.draws.clear();
        self.vertices.clear();
        self.current_frame += 1;
    }

    /// Return `(width, height)` of the cached/loaded texture, or `None` on failure.
    pub unsafe fn texture_size(&mut self, path: &str) -> Option<(u32, u32)> {
        let slot = self.find_or_load(path)?;
        let tex = self.cache[slot].as_ref()?;
        Some((tex.width, tex.height))
    }

    /// Schedule a textured quad at (x, y, w, h).
    ///
    /// If `path` is already cached the texture is reused; otherwise the image
    /// file is loaded, uploaded to the GPU, and placed in the least-recently-
    /// used cache slot.
    pub unsafe fn prepare_image(
        &mut self,
        path: &str,
        x: f32, y: f32, width: f32, height: f32,
    ) {
        if self.draws.len() >= MAX_CACHED_IMAGES { return; }

        let slot = match self.find_or_load(path) {
            Some(s) => s,
            None => return,
        };

        let vertex_offset = self.vertices.len() as u32;

        let (x0, y0, x1, y1) = (x, y, x + width, y + height);
        // Two triangles covering the quad, UV (0,0)→(1,1)
        self.vertices.push(ImageVertex { pos: [x0, y0], tex_coord: [0.0, 0.0] });
        self.vertices.push(ImageVertex { pos: [x1, y0], tex_coord: [1.0, 0.0] });
        self.vertices.push(ImageVertex { pos: [x1, y1], tex_coord: [1.0, 1.0] });
        self.vertices.push(ImageVertex { pos: [x0, y0], tex_coord: [0.0, 0.0] });
        self.vertices.push(ImageVertex { pos: [x1, y1], tex_coord: [1.0, 1.0] });
        self.vertices.push(ImageVertex { pos: [x0, y1], tex_coord: [0.0, 1.0] });

        self.draws.push(ImageDraw { slot, vertex_offset });
    }

    /// Upload vertices and issue one draw call per queued image.
    pub unsafe fn draw_images(
        &self,
        device: &ash::Device,
        cb: vk::CommandBuffer,
        extent: vk::Extent2D,
    ) {
        if self.draws.is_empty() { return; }

        let upload_size = (std::mem::size_of::<ImageVertex>() * self.vertices.len()) as vk::DeviceSize;
        let ptr = device
            .map_memory(self.vertex_buffer_memory, 0, upload_size, vk::MemoryMapFlags::empty())
            .unwrap() as *mut ImageVertex;
        ptr::copy_nonoverlapping(self.vertices.as_ptr(), ptr, self.vertices.len());
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

        for draw in &self.draws {
            device.cmd_bind_descriptor_sets(
                cb,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_sets[draw.slot]],
                &[],
            );
            device.cmd_draw(cb, VERTS_PER_QUAD as u32, 1, draw.vertex_offset, 0);
        }
    }

    // ---- Cleanup -------------------------------------------------------------

    pub unsafe fn cleanup(&mut self) {
        for slot in self.cache.iter_mut() {
            if let Some(tex) = slot.take() {
                self.device.destroy_sampler(tex.sampler, None);
                self.device.destroy_image_view(tex.view, None);
                self.device.destroy_image(tex.image, None);
                self.device.free_memory(tex.memory, None);
            }
        }
        self.device.destroy_pipeline(self.pipeline, None);
        self.device.destroy_pipeline_layout(self.pipeline_layout, None);
        self.device.destroy_descriptor_pool(self.descriptor_pool, None);
        self.device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        self.device.destroy_buffer(self.vertex_buffer, None);
        self.device.free_memory(self.vertex_buffer_memory, None);
    }

    // ---- Internal helpers ----------------------------------------------------

    /// Return the cache slot index for `path`, loading the texture if needed.
    unsafe fn find_or_load(&mut self, path: &str) -> Option<usize> {
        // Check if already cached
        for (i, slot) in self.cache.iter_mut().enumerate() {
            if let Some(tex) = slot {
                if tex.path == path {
                    tex.last_used_frame = self.current_frame;
                    return Some(i);
                }
            }
        }

        // Find a free slot or the LRU slot
        let evict_slot = self.find_evict_slot();

        // Evict existing texture if necessary
        if let Some(old) = self.cache[evict_slot].take() {
            self.device.destroy_sampler(old.sampler, None);
            self.device.destroy_image_view(old.view, None);
            self.device.destroy_image(old.image, None);
            self.device.free_memory(old.memory, None);
        }

        // Load new texture
        match self.load_texture(path) {
            Ok(tex) => {
                // Update the pre-allocated descriptor set for this slot
                let image_info = vk::DescriptorImageInfo::default()
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .image_view(tex.view)
                    .sampler(tex.sampler);
                let write = vk::WriteDescriptorSet::default()
                    .dst_set(self.descriptor_sets[evict_slot])
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(std::slice::from_ref(&image_info));
                self.device.update_descriptor_sets(&[write], &[]);

                self.cache[evict_slot] = Some(tex);
                Some(evict_slot)
            }
            Err(e) => {
                eprintln!("sicompass: image load failed for '{path}': {e}");
                None
            }
        }
    }

    /// Choose the slot to evict: prefer empty slots, then pick the LRU.
    fn find_evict_slot(&self) -> usize {
        find_evict_slot_in(&self.cache)
    }

    /// Decode an image file and upload it to a new `VkImage`.
    unsafe fn load_texture(&self, path: &str) -> Result<CachedTexture, SiError> {
        // Decode via the `image` crate (supports PNG, JPEG, WebP, …)
        let dyn_img = img_crate::open(path)
            .map_err(|e| SiError::Other(format!("image decode: {e}")))?;
        let (width, height) = dyn_img.dimensions();
        let rgba = dyn_img.into_rgba8();
        let pixel_bytes: &[u8] = &rgba;

        // Staging buffer
        let buf_size = pixel_bytes.len() as vk::DeviceSize;
        let (staging_buf, staging_mem) = render::create_buffer(
            &self.device,
            &self.instance,
            self.physical_device,
            buf_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let ptr = self.device
            .map_memory(staging_mem, 0, buf_size, vk::MemoryMapFlags::empty())?
            as *mut u8;
        ptr::copy_nonoverlapping(pixel_bytes.as_ptr(), ptr, pixel_bytes.len());
        self.device.unmap_memory(staging_mem);

        // Device-local VkImage (R8G8B8A8_SRGB)
        let (image, memory) = render::create_image_helper(
            &self.device,
            &self.instance,
            self.physical_device,
            width,
            height,
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        // Transition → copy → transition
        render::transition_image_layout(
            &self.device, self.command_pool, self.queue,
            image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
        render::copy_buffer_to_image(
            &self.device, self.command_pool, self.queue,
            staging_buf, image, width, height,
        );
        render::transition_image_layout(
            &self.device, self.command_pool, self.queue,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );

        // Staging buffer no longer needed
        self.device.destroy_buffer(staging_buf, None);
        self.device.free_memory(staging_mem, None);

        // Image view
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_SRGB)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0).level_count(1)
                    .base_array_layer(0).layer_count(1),
            );
        let view = self.device.create_image_view(&view_info, None)?;

        // Sampler (linear filter, clamp to edge)
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .anisotropy_enable(false)
            .unnormalized_coordinates(false);
        let sampler = self.device.create_sampler(&sampler_info, None)?;

        Ok(CachedTexture {
            image,
            memory,
            view,
            sampler,
            path: path.to_owned(),
            width,
            height,
            last_used_frame: self.current_frame,
        })
    }
}

// ---------------------------------------------------------------------------
// Standalone helpers (pub(crate) for testability)
// ---------------------------------------------------------------------------

/// Choose the cache slot to evict: prefer empty slots, then pick the LRU.
pub(crate) fn find_evict_slot_in(cache: &[Option<CachedTexture>]) -> usize {
    if let Some(i) = cache.iter().position(|s| s.is_none()) {
        return i;
    }
    cache
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|t| (i, t.last_used_frame)))
        .min_by_key(|&(_, f)| f)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_evict_slot_prefers_empty() {
        let mut cache: Vec<Option<CachedTexture>> = (0..MAX_CACHED_IMAGES).map(|_| None).collect();
        cache[0] = Some(dummy_cached(0, 5));
        cache[1] = Some(dummy_cached(1, 3));
        // Slots 2.. remain None
        let slot = find_evict_slot_in(&cache);
        assert!(slot >= 2, "should prefer an empty slot, got {slot}");
    }

    #[test]
    fn find_evict_slot_lru_when_full() {
        let cache: Vec<Option<CachedTexture>> = (0..MAX_CACHED_IMAGES)
            .map(|i| Some(dummy_cached(i, (i as u64 + 1) * 10)))
            .collect();
        let slot = find_evict_slot_in(&cache);
        // Slot 0 has last_used_frame = 10 — the smallest
        assert_eq!(slot, 0);
    }

    #[test]
    fn find_evict_slot_empty_cache_returns_zero() {
        let cache: Vec<Option<CachedTexture>> = (0..MAX_CACHED_IMAGES).map(|_| None).collect();
        let slot = find_evict_slot_in(&cache);
        assert_eq!(slot, 0);
    }

    fn dummy_cached(idx: usize, last_used: u64) -> CachedTexture {
        CachedTexture {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sampler: vk::Sampler::null(),
            path: format!("dummy_{idx}.png"),
            width: 1,
            height: 1,
            last_used_frame: last_used,
        }
    }
}
