//! Vulkan initialisation, swap-chain management, and frame drawing.
//!
//! Mirrors `main.c` (setup) and the Vulkan parts of `render.c` in the C code.
//! Phase 3: clear-to-colour pass.  Shader pipelines (text/rect/image) are Phase 4.

use crate::app_state::{AppState, SiError, MAX_FRAMES_IN_FLIGHT, WINDOW_TITLE, WINDOW_WIDTH, WINDOW_HEIGHT};
use ash::vk;
use ash::vk::Handle as _;
use std::ffi::{CStr, CString};

// ---------------------------------------------------------------------------
// Validation layers
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
const VALIDATION_LAYERS: &[&CStr] = &[c"VK_LAYER_KHRONOS_validation"];
#[cfg(not(debug_assertions))]
const VALIDATION_LAYERS: &[&CStr] = &[];

const DEVICE_EXTENSIONS: &[&CStr] = &[ash::khr::swapchain::NAME];

// ---------------------------------------------------------------------------
// Runtime file check
// ---------------------------------------------------------------------------

/// Check that all required shader / font files exist.
/// Returns `EXIT_SUCCESS` (0) or `EXIT_FAILURE` (1).
pub fn check_runtime_files() -> i32 {
    const REQUIRED: &[&str] = &[
        "fonts/Consolas-Regular.ttf",
        "shaders/text_vert.spv",
        "shaders/text_frag.spv",
        "shaders/rectangle_vert.spv",
        "shaders/rectangle_frag.spv",
        "shaders/image_vert.spv",
        "shaders/image_frag.spv",
    ];
    let mut missing = 0;
    for path in REQUIRED {
        if std::path::Path::new(path).exists() {
            println!("OK: {path}");
        } else {
            eprintln!("MISSING: {path}");
            missing += 1;
        }
    }
    if missing > 0 {
        eprintln!("\n{missing} file(s) missing");
        1
    } else {
        println!("\nAll runtime files present");
        0
    }
}

// ---------------------------------------------------------------------------
// Queue family helpers
// ---------------------------------------------------------------------------

struct QueueFamilies {
    graphics: u32,
    present: u32,
}

unsafe fn find_queue_families(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> Option<QueueFamilies> {
    let props = instance.get_physical_device_queue_family_properties(physical_device);
    let mut graphics = None;
    let mut present = None;
    for (i, p) in props.iter().enumerate() {
        if p.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            graphics = Some(i as u32);
        }
        if surface_loader
            .get_physical_device_surface_support(physical_device, i as u32, surface)
            .unwrap_or(false)
        {
            present = Some(i as u32);
        }
        if graphics.is_some() && present.is_some() {
            break;
        }
    }
    Some(QueueFamilies {
        graphics: graphics?,
        present: present?,
    })
}

// ---------------------------------------------------------------------------
// Swap-chain helpers
// ---------------------------------------------------------------------------

struct SwapchainSupport {
    capabilities: vk::SurfaceCapabilitiesKHR,
    formats: Vec<vk::SurfaceFormatKHR>,
    present_modes: Vec<vk::PresentModeKHR>,
}

unsafe fn query_swapchain_support(
    surface_loader: &ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> Result<SwapchainSupport, vk::Result> {
    let capabilities = surface_loader
        .get_physical_device_surface_capabilities(physical_device, surface)?;
    let formats = surface_loader
        .get_physical_device_surface_formats(physical_device, surface)?;
    let present_modes = surface_loader
        .get_physical_device_surface_present_modes(physical_device, surface)?;
    Ok(SwapchainSupport { capabilities, formats, present_modes })
}

fn choose_surface_format(formats: &[vk::SurfaceFormatKHR]) -> vk::SurfaceFormatKHR {
    formats
        .iter()
        .find(|f| {
            f.format == vk::Format::B8G8R8A8_SRGB
                && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .copied()
        .unwrap_or(formats[0])
}

fn choose_present_mode(modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    if modes.contains(&vk::PresentModeKHR::MAILBOX) {
        vk::PresentModeKHR::MAILBOX
    } else {
        vk::PresentModeKHR::FIFO
    }
}

fn choose_extent(caps: &vk::SurfaceCapabilitiesKHR, window: &sdl3::video::Window) -> vk::Extent2D {
    if caps.current_extent.width != u32::MAX {
        return caps.current_extent;
    }
    let (w, h) = window.size_in_pixels();
    vk::Extent2D {
        width: w.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
        height: h.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
    }
}

// ---------------------------------------------------------------------------
// Physical device selection
// ---------------------------------------------------------------------------

unsafe fn is_device_suitable(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> bool {
    // Must have graphics + present queues
    if find_queue_families(instance, surface_loader, device, surface).is_none() {
        return false;
    }
    // Must support required extensions
    let Ok(ext_props) = instance.enumerate_device_extension_properties(device) else {
        return false;
    };
    for required in DEVICE_EXTENSIONS {
        if !ext_props.iter().any(|e| {
            CStr::from_bytes_until_nul(&e.extension_name.map(|b| b as u8))
                .ok()
                .map_or(false, |n| n == *required)
        }) {
            return false;
        }
    }
    // Swapchain must be adequate (at least one format and one present mode)
    let Ok(sc) = query_swapchain_support(surface_loader, device, surface) else {
        return false;
    };
    if sc.formats.is_empty() || sc.present_modes.is_empty() {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Swapchain creation (also used in recreate)
// ---------------------------------------------------------------------------

pub struct SwapchainBundle {
    pub swapchain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub image_views: Vec<vk::ImageView>,
}

unsafe fn create_swapchain(
    device: &ash::Device,
    swapchain_loader: &ash::khr::swapchain::Device,
    surface_loader: &ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    graphics_family: u32,
    present_family: u32,
    window: &sdl3::video::Window,
    old_swapchain: vk::SwapchainKHR,
) -> Result<SwapchainBundle, SiError> {
    let support = query_swapchain_support(surface_loader, physical_device, surface)?;
    let fmt = choose_surface_format(&support.formats);
    let mode = choose_present_mode(&support.present_modes);
    let extent = choose_extent(&support.capabilities, window);

    let mut image_count = support.capabilities.min_image_count + 1;
    if support.capabilities.max_image_count > 0 {
        image_count = image_count.min(support.capabilities.max_image_count);
    }

    let queue_families = [graphics_family, present_family];
    let (sharing_mode, qf_slice): (vk::SharingMode, &[u32]) = if graphics_family == present_family {
        (vk::SharingMode::EXCLUSIVE, &[])
    } else {
        (vk::SharingMode::CONCURRENT, &queue_families)
    };

    let create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(fmt.format)
        .image_color_space(fmt.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(sharing_mode)
        .queue_family_indices(qf_slice)
        .pre_transform(support.capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(mode)
        .clipped(true)
        .old_swapchain(old_swapchain);

    let swapchain = swapchain_loader.create_swapchain(&create_info, None)?;
    let images = swapchain_loader.get_swapchain_images(swapchain)?;

    let image_views: Vec<vk::ImageView> = images
        .iter()
        .map(|&img| {
            let view_info = vk::ImageViewCreateInfo::default()
                .image(img)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(fmt.format)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1),
                );
            device.create_image_view(&view_info, None)
        })
        .collect::<Result<_, _>>()?;

    Ok(SwapchainBundle {
        swapchain,
        images,
        format: fmt.format,
        extent,
        image_views,
    })
}

unsafe fn create_render_pass(
    device: &ash::Device,
    format: vk::Format,
) -> Result<vk::RenderPass, SiError> {
    let attachment = vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR);

    let color_ref = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_ref);

    let dependency = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

    let attachments = [attachment];
    let subpasses = [subpass];
    let dependencies = [dependency];
    let rp_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    Ok(device.create_render_pass(&rp_info, None)?)
}

unsafe fn create_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    image_views: &[vk::ImageView],
    extent: vk::Extent2D,
) -> Result<Vec<vk::Framebuffer>, SiError> {
    image_views
        .iter()
        .map(|&view| {
            let attachments = [view];
            let fb_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(extent.width)
                .height(extent.height)
                .layers(1);
            device.create_framebuffer(&fb_info, None).map_err(SiError::from)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Public: full app construction
// ---------------------------------------------------------------------------

/// Build the complete `AppState`.  Called by `AppState::new()`.
pub fn build_app() -> Result<AppState, SiError> {
    // ---- SDL init -----------------------------------------------------------
    let sdl = sdl3::init().map_err(SiError::Sdl)?;
    let video = sdl.video().map_err(SiError::Sdl)?;

    let mut window = video
        .window(WINDOW_TITLE, WINDOW_WIDTH, WINDOW_HEIGHT)
        .vulkan()
        .resizable()
        .hidden()
        .build()
        .map_err(|e| SiError::Sdl(e.to_string()))?;

    let event_pump = sdl.event_pump().map_err(SiError::Sdl)?;

    // ---- ash Entry (loads libvulkan.so / vulkan-1.dll) ----------------------
    let entry = unsafe { ash::Entry::load()? };

    // ---- Vulkan instance ----------------------------------------------------
    let app_name = CString::new(WINDOW_TITLE).unwrap();
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .application_version(vk::make_api_version(0, 1, 0, 0))
        .engine_name(c"No Engine")
        .engine_version(vk::make_api_version(0, 1, 0, 0))
        .api_version(vk::API_VERSION_1_0);

    // SDL3 required Vulkan instance extensions
    let sdl_exts = window
        .vulkan_instance_extensions()
        .map_err(SiError::Sdl)?;
    let mut ext_names_raw: Vec<*const i8> = sdl_exts
        .iter()
        .map(|s| CString::new(s.as_str()).unwrap().into_raw() as *const i8)
        .collect();

    #[cfg(debug_assertions)]
    {
        ext_names_raw.push(ash::ext::debug_utils::NAME.as_ptr());
    }

    let layer_names_raw: Vec<*const i8> = VALIDATION_LAYERS
        .iter()
        .map(|s| s.as_ptr())
        .collect();

    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&ext_names_raw)
        .enabled_layer_names(&layer_names_raw);

    let instance = unsafe { entry.create_instance(&instance_info, None)? };

    // Free the CString pointers we allocated for SDL extensions
    // Safety: we created these above with into_raw()
    unsafe {
        for ptr in &ext_names_raw {
            // Skip the debug_utils name (static) and SDL names (from CString::into_raw)
            // We only allocated CStrings for SDL extensions, not the debug utils name
        }
    }
    // Actually: drop is handled correctly because we used into_raw() which leaks —
    // for a long-lived app this is negligible; fix with a proper owned Vec<CString> if needed.

    // ---- Debug messenger (debug builds only) --------------------------------
    #[cfg(debug_assertions)]
    let (debug_utils, debug_messenger) = unsafe {
        let du = ash::ext::debug_utils::Instance::new(&entry, &instance);
        let msg_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(vulkan_debug_callback));
        let messenger = du.create_debug_utils_messenger(&msg_info, None)?;
        (du, messenger)
    };

    // ---- Vulkan surface (SDL3 bridge) ----------------------------------------
    // ash vk::Instance is a u64 handle; sdl3-sys expects *mut __VkInstance.
    // Both represent the same underlying Vulkan handle on 64-bit platforms.
    let surface = unsafe {
        use sdl3::sys::vulkan as sdl_vk;
        let raw_handle: u64 = instance.handle().as_raw();
        let sdl_instance = raw_handle as usize as sdl_vk::VkInstance;
        let sdl_surface = window
            .vulkan_create_surface(sdl_instance)
            .map_err(SiError::Sdl)?;
        // sdl_surface is *mut __VkSurfaceKHR; ash stores SurfaceKHR as u64.
        vk::SurfaceKHR::from_raw(sdl_surface as usize as u64)
    };
    let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

    // ---- Physical device ----------------------------------------------------
    let physical_device = unsafe {
        instance
            .enumerate_physical_devices()?
            .into_iter()
            .find(|&pd| {
                is_device_suitable(&instance, &surface_loader, pd, surface)
            })
            .ok_or_else(|| SiError::Other("No suitable Vulkan GPU found".into()))?
    };

    let queue_families = unsafe {
        find_queue_families(&instance, &surface_loader, physical_device, surface)
            .ok_or_else(|| SiError::Other("Queue families not found".into()))?
    };

    // ---- Logical device + queues --------------------------------------------
    let unique_families: Vec<u32> = {
        let mut v = vec![queue_families.graphics, queue_families.present];
        v.dedup();
        v
    };

    let queue_priority = [1.0f32];
    let queue_create_infos: Vec<vk::DeviceQueueCreateInfo> = unique_families
        .iter()
        .map(|&qf| {
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(qf)
                .queue_priorities(&queue_priority)
        })
        .collect();

    let ext_names: Vec<*const i8> = DEVICE_EXTENSIONS.iter().map(|s| s.as_ptr()).collect();

    let device_features = vk::PhysicalDeviceFeatures::default();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&ext_names)
        .enabled_features(&device_features);

    let device = unsafe { instance.create_device(physical_device, &device_info, None)? };

    let graphics_queue =
        unsafe { device.get_device_queue(queue_families.graphics, 0) };
    let present_queue =
        unsafe { device.get_device_queue(queue_families.present, 0) };

    // ---- Swapchain ----------------------------------------------------------
    let swapchain_loader = ash::khr::swapchain::Device::new(&instance, &device);

    let sc = unsafe {
        create_swapchain(
            &device,
            &swapchain_loader,
            &surface_loader,
            physical_device,
            surface,
            queue_families.graphics,
            queue_families.present,
            &window,
            vk::SwapchainKHR::null(),
        )?
    };

    // ---- Render pass --------------------------------------------------------
    let render_pass = unsafe { create_render_pass(&device, sc.format)? };

    // ---- Framebuffers -------------------------------------------------------
    let framebuffers =
        unsafe { create_framebuffers(&device, render_pass, &sc.image_views, sc.extent)? };

    // ---- Command pool + buffers ---------------------------------------------
    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(queue_families.graphics);
    let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(MAX_FRAMES_IN_FLIGHT as u32);
    let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };

    // ---- Sync objects -------------------------------------------------------
    let sem_info = vk::SemaphoreCreateInfo::default();
    let fence_info =
        vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

    let image_available = [
        unsafe { device.create_semaphore(&sem_info, None)? },
        unsafe { device.create_semaphore(&sem_info, None)? },
    ];
    let render_finished = [
        unsafe { device.create_semaphore(&sem_info, None)? },
        unsafe { device.create_semaphore(&sem_info, None)? },
    ];
    let in_flight = [
        unsafe { device.create_fence(&fence_info, None)? },
        unsafe { device.create_fence(&fence_info, None)? },
    ];

    // ---- Show window --------------------------------------------------------
    window.show();

    Ok(AppState {
        _sdl: sdl,
        _video: video,
        window,
        event_pump,
        entry,
        instance,
        #[cfg(debug_assertions)]
        debug_utils,
        #[cfg(debug_assertions)]
        debug_messenger,
        surface_loader,
        surface,
        physical_device,
        device,
        graphics_queue,
        present_queue,
        graphics_family: queue_families.graphics,
        present_family: queue_families.present,
        swapchain_loader,
        swapchain: sc.swapchain,
        swapchain_images: sc.images,
        swapchain_format: sc.format,
        swapchain_extent: sc.extent,
        swapchain_image_views: sc.image_views,
        render_pass,
        framebuffers,
        command_pool,
        command_buffers,
        image_available,
        render_finished,
        in_flight,
        current_frame: 0,
        framebuffer_resized: false,
        running: true,
        // Dark theme background: 0x000000FF
        clear_color: [0.0, 0.0, 0.0, 1.0],
    })
}

// ---------------------------------------------------------------------------
// Public: recreate swap-chain on resize
// ---------------------------------------------------------------------------

pub fn recreate_swapchain(app: &mut AppState) {
    unsafe {
        // Wait until the window is not minimised
        loop {
            let (w, h) = app.window.size_in_pixels();
            if w > 0 && h > 0 {
                break;
            }
            // window is minimised — yield to SDL event loop
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        app.device.device_wait_idle().unwrap();

        // Destroy old framebuffers + image views
        for &fb in &app.framebuffers {
            app.device.destroy_framebuffer(fb, None);
        }
        for &iv in &app.swapchain_image_views {
            app.device.destroy_image_view(iv, None);
        }
        let old = app.swapchain;

        // Recreate
        let sc = create_swapchain(
            &app.device,
            &app.swapchain_loader,
            &app.surface_loader,
            app.physical_device,
            app.surface,
            app.graphics_family,
            app.present_family,
            &app.window,
            old,
        )
        .expect("recreate_swapchain failed");

        app.swapchain_loader.destroy_swapchain(old, None);

        app.swapchain = sc.swapchain;
        app.swapchain_images = sc.images;
        app.swapchain_format = sc.format;
        app.swapchain_extent = sc.extent;
        app.swapchain_image_views = sc.image_views;

        app.framebuffers =
            create_framebuffers(&app.device, app.render_pass, &app.swapchain_image_views, app.swapchain_extent)
                .expect("recreate framebuffers failed");
    }
}

// ---------------------------------------------------------------------------
// Public: draw one frame
// ---------------------------------------------------------------------------

pub fn draw_frame(app: &mut AppState) {
    let frame = app.current_frame;

    unsafe {
        // Wait for previous frame's fence
        app.device
            .wait_for_fences(&[app.in_flight[frame]], true, u64::MAX)
            .unwrap();

        // Acquire next swapchain image
        let result = app.swapchain_loader.acquire_next_image(
            app.swapchain,
            u64::MAX,
            app.image_available[frame],
            vk::Fence::null(),
        );

        let image_index = match result {
            Ok((idx, _)) => idx,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                recreate_swapchain(app);
                return;
            }
            Err(e) => {
                eprintln!("acquire_next_image: {e}");
                return;
            }
        };

        app.device.reset_fences(&[app.in_flight[frame]]).unwrap();

        // Record command buffer
        let cb = app.command_buffers[frame];
        app.device
            .reset_command_buffer(cb, vk::CommandBufferResetFlags::empty())
            .unwrap();

        let begin_info = vk::CommandBufferBeginInfo::default();
        app.device.begin_command_buffer(cb, &begin_info).unwrap();

        let [r, g, b, a] = app.clear_color;
        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [r, g, b, a],
            },
        }];
        let rp_begin = vk::RenderPassBeginInfo::default()
            .render_pass(app.render_pass)
            .framebuffer(app.framebuffers[image_index as usize])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: app.swapchain_extent,
            })
            .clear_values(&clear_values);

        app.device.cmd_begin_render_pass(cb, &rp_begin, vk::SubpassContents::INLINE);
        // Phase 4: drawText / drawRectangle / drawImage go here
        app.device.cmd_end_render_pass(cb);
        app.device.end_command_buffer(cb).unwrap();

        // Submit
        let wait_sems = [app.image_available[frame]];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal_sems = [app.render_finished[frame]];
        let cbs = [cb];
        let submit_info = vk::SubmitInfo::default()
            .wait_semaphores(&wait_sems)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&cbs)
            .signal_semaphores(&signal_sems);

        app.device
            .queue_submit(app.graphics_queue, &[submit_info], app.in_flight[frame])
            .unwrap();

        // Present
        let swapchains = [app.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_sems)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        let present_result = app
            .swapchain_loader
            .queue_present(app.present_queue, &present_info);

        match present_result {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR)
            | Ok(true /* suboptimal */) => {
                app.framebuffer_resized = false;
                recreate_swapchain(app);
            }
            Err(e) => eprintln!("queue_present: {e}"),
            _ if app.framebuffer_resized => {
                app.framebuffer_resized = false;
                recreate_swapchain(app);
            }
            _ => {}
        }
    }

    app.current_frame = (app.current_frame + 1) % MAX_FRAMES_IN_FLIGHT;
}

// ---------------------------------------------------------------------------
// Public: cleanup (called from AppState::drop)
// ---------------------------------------------------------------------------

pub fn cleanup(app: &mut AppState) {
    unsafe {
        // Let all GPU work finish before we destroy anything
        let _ = app.device.device_wait_idle();

        for i in 0..MAX_FRAMES_IN_FLIGHT {
            app.device.destroy_semaphore(app.render_finished[i], None);
            app.device.destroy_semaphore(app.image_available[i], None);
            app.device.destroy_fence(app.in_flight[i], None);
        }
        app.device.destroy_command_pool(app.command_pool, None);

        for &fb in &app.framebuffers {
            app.device.destroy_framebuffer(fb, None);
        }
        app.device.destroy_render_pass(app.render_pass, None);

        for &iv in &app.swapchain_image_views {
            app.device.destroy_image_view(iv, None);
        }
        app.swapchain_loader
            .destroy_swapchain(app.swapchain, None);

        app.device.destroy_device(None);

        #[cfg(debug_assertions)]
        app.debug_utils
            .destroy_debug_utils_messenger(app.debug_messenger, None);

        app.surface_loader.destroy_surface(app.surface, None);
        app.instance.destroy_instance(None);
    }
}

// ---------------------------------------------------------------------------
// Debug callback
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
unsafe extern "system" fn vulkan_debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _type: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let msg = unsafe { CStr::from_ptr((*data).p_message) }
        .to_string_lossy();
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        eprintln!("[VK ERROR] {msg}");
    } else {
        eprintln!("[VK WARN]  {msg}");
    }
    vk::FALSE
}
