use crate::graphics::*;
use crate::{log, project};
use ash::vk;

#[repr(C)]
pub struct Vertex
{
    pub position:  [f32; 3],
    pub colour:    [f32; 3],
    pub tex_coord: [f32; 2],
}

pub const VERTICES: [Vertex; 4] = [
    Vertex {
        position:  [-0.5, -0.5, 0.0],
        colour:    [1.0, 0.0, 0.0],
        tex_coord: [1.0, 0.0],
    },
    Vertex {
        position:  [0.5, -0.5, 0.0],
        colour:    [0.0, 1.0, 0.0],
        tex_coord: [0.0, 0.0],
    },
    Vertex {
        position:  [0.5, 0.5, 0.0],
        colour:    [0.0, 0.0, 1.0],
        tex_coord: [0.0, 1.0],
    },
    Vertex {
        position:  [-0.5, 0.5, 0.0],
        colour:    [1.0, 1.0, 1.0],
        tex_coord: [1.0, 1.0],
    },
];

pub const INDICES: [u16; 6] = [0, 1, 2, 2, 3, 0];

pub type Result<T> = std::result::Result<T, errors::VkAppError>;

pub struct VkApp
{
    _entry:                 ash::Entry, // For loading vulkan, must have same lifetime as struct
    instance:               ash::Instance,
    debug_utils_loader:     ash::ext::debug_utils::Instance,
    debug_callback:         vk::DebugUtilsMessengerEXT,
    physical_device:        device::SupportedPhysicalDevice,
    surface:                presentation::Surface,
    device:                 ash::Device,
    graphics_queue:         vk::Queue,
    present_queue:          vk::Queue,
    swapchain:              presentation::Swapchain,
    pipeline:               pipeline::Pipeline,
    command_pool:           vk::CommandPool,
    texture_image:          vk::Image,
    texture_image_memory:   vk::DeviceMemory,
    texture_image_view:     vk::ImageView,
    texture_sampler:        vk::Sampler,
    vertex_buffer:          buffers::Buffer,
    index_buffer:           buffers::Buffer,
    uniform_buffers:        Vec<buffers::Buffer>,
    uniform_buffers_mapped: Vec<*mut std::ffi::c_void>,
    descriptor_pool:        vk::DescriptorPool,
    descriptor_sets:        Vec<vk::DescriptorSet>,
    command_buffers:        Vec<vk::CommandBuffer>,
    sync_objects:           commands::SyncObjects,
    // current_frame keeps track of the index to use the right objects (command buffers, semaphores)
    current_frame:          usize,
}

impl Drop for VkApp
{
    fn drop(&mut self)
    {
        log!("Cleaning up VkApp");
        unsafe {
            if self.device.handle() != vk::Device::null() {
                self.device.device_wait_idle().unwrap(); // TODO should be unwrap?
            }

            self.swapchain.cleanup(&self.device);

            self.device.destroy_sampler(self.texture_sampler, None);
            self.device.destroy_image_view(self.texture_image_view, None);
            self.device.destroy_image(self.texture_image, None);
            self.device.free_memory(self.texture_image_memory, None);

            for uniform_buffer in &self.uniform_buffers {
                uniform_buffer.cleanup(&self.device);
            }

            self.device.destroy_descriptor_pool(self.descriptor_pool, None);

            self.vertex_buffer.cleanup(&self.device);
            self.index_buffer.cleanup(&self.device);

            self.pipeline.cleanup(&self.device);
            self.sync_objects.cleanup(&self.device);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.surface.loader.destroy_surface(self.surface.vk_surface, None);
            self.debug_utils_loader
                .destroy_debug_utils_messenger(self.debug_callback, None);
            self.instance.destroy_instance(None);
        }
        log!("Complete");
    }
}

impl VkApp
{
    pub fn new(hwnd: &windows::Win32::Foundation::HWND, h_instance: &windows::Win32::Foundation::HINSTANCE) -> Result<Self>
    {
        //let entry = unsafe { ash::Entry::load().unwrap() };
        let entry = ash::Entry::linked(); // Dev only
        let instance = device::create_instance(&entry)?;
        let (debug_utils_loader, debug_callback) = device::create_debug_messenger(&entry, &instance)?;
        let (surface_loader, vk_surface) = presentation::create_surface(&entry, &instance, hwnd, h_instance)?;

        // Just get the first device
        let (physical_device, surface_details) =
            match device::get_physical_devices(&instance, &surface_loader, vk_surface)?.get(0) {
                Some((physical_device, surface_details)) => {
                    log!("Selected device {}", physical_device.device_name);
                    (physical_device.to_owned(), surface_details.to_owned())
                }
                None => return Err(errors::VkAppError::DeviceError(String::from("No supported devices"))),
            };

        let device = device::create_logical_device(&instance, &physical_device)?;

        let surface = presentation::Surface { loader: surface_loader, vk_surface, details: surface_details };

        let (graphics_queue, present_queue) = unsafe {
            (
                device.get_device_queue(physical_device.graphics_family_index, 0),
                device.get_device_queue(physical_device.present_family_index, 0),
            )
        };

        let mut swapchain = presentation::create_swapchain(&instance, &device, &physical_device, &surface)?;
        let pipeline = pipeline::create_pipeline(&device, swapchain.settings)?;
        swapchain.create_framebuffers(&device, &pipeline)?;

        let command_pool = commands::create_command_pool(&device, physical_device.graphics_family_index)?;

        let (texture_image, texture_image_memory) = textures::create_texture_image(
            &instance,
            physical_device.vk_physical_device,
            &device,
            command_pool,
            graphics_queue,
            "cobble1.png",
        )?;

        let texture_image_view = textures::create_texture_image_view(&device, texture_image)?;

        let texture_sampler = textures::create_texture_sampler(&instance, &device, physical_device.vk_physical_device)?;

        let vertex_buffer = buffers::create_vertex_buffer(
            &instance,
            physical_device.vk_physical_device,
            &device,
            command_pool,
            graphics_queue,
        )?;

        let index_buffer = buffers::create_index_buffer(
            &instance,
            physical_device.vk_physical_device,
            &device,
            command_pool,
            graphics_queue,
        )?;

        let (uniform_buffers, uniform_buffers_mapped) =
            buffers::create_uniform_buffers(&instance, physical_device.vk_physical_device, &device)?;

        let descriptor_pool = buffers::create_descriptor_pool(&device)?;

        let descriptor_sets = buffers::create_descriptor_sets(
            &device,
            descriptor_pool,
            &uniform_buffers,
            pipeline.descriptor_set_layout,
            texture_image_view,
            texture_sampler,
        )?;

        let command_buffers = commands::create_command_buffers(&device, command_pool)?;

        let sync_objects = commands::create_sync_objects(&device)?;

        Ok(Self {
            _entry: entry,
            instance,
            debug_utils_loader,
            debug_callback,
            surface,
            physical_device,
            device,
            graphics_queue,
            present_queue,
            swapchain,
            pipeline,
            command_pool,
            texture_image,
            texture_image_memory,
            texture_image_view,
            texture_sampler,
            vertex_buffer,
            index_buffer,
            uniform_buffers,
            uniform_buffers_mapped,
            descriptor_pool,
            descriptor_sets,
            command_buffers,
            sync_objects,
            current_frame: 0,
        })
    }

    pub fn draw_frame(&mut self) -> Result<()>
    {
        unsafe {
            // Wait until the current previous frame has finished
            self.device
                .wait_for_fences(&[self.sync_objects.in_flight_fences[self.current_frame]], true, u64::MAX)?;

            // Acquire an image from the swapchain
            let (image_index, suboptimal_surface) = match self.swapchain.swapchain_device.acquire_next_image(
                self.swapchain.vk_swapchain,
                u64::MAX, // Disable timeout for images to become available
                self.sync_objects.image_available_semaphores[self.current_frame], // Synchronization object for when presentation execution has finished using the image
                vk::Fence::null(),
            ) {
                Ok((image_index, suboptimal_surface)) => (image_index, suboptimal_surface),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    // Swapchain has become incompatible with surface and can no longer be used for rendering, must be recreated and try again in next draw
                    self.recreate_swapchain()?;
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            };

            buffers::update_uniform_buffer(&self.uniform_buffers_mapped, self.current_frame);

            // Only reset the fence if we are sure we are submitting work to prevent deadlock
            self.device
                .reset_fences(&[self.sync_objects.in_flight_fences[self.current_frame]])?;

            self.device
                .reset_command_buffer(self.command_buffers[self.current_frame], vk::CommandBufferResetFlags::empty())?;

            commands::record_command_buffer(
                &self.device,
                self.command_buffers[self.current_frame],
                image_index,
                &self.pipeline,
                &self.swapchain,
                self.vertex_buffer.buffer,
                self.index_buffer.buffer,
                vec![self.descriptor_sets[self.current_frame]],
            )?;

            // Semaphores to wait on before execution begins
            let wait_semaphores: [vk::Semaphore; 1] = [self.sync_objects.image_available_semaphores[self.current_frame]];
            // Which stage of the pipeline to wait on. We wait at the point of writing colours to the image until its available
            let wait_stages: [vk::PipelineStageFlags; 1] = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            // Which semaphores to signal once the command buffer has finished execution
            let signal_semaphores: [vk::Semaphore; 1] = [self.sync_objects.render_finished_semaphores[self.current_frame]];

            let command_buffers = [self.command_buffers[self.current_frame]];

            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores);

            self.device.queue_submit(
                self.graphics_queue,
                [submit_info].as_slice(),
                self.sync_objects.in_flight_fences[self.current_frame],
            )?;

            // Finally, submit the result of the render pass back to the swapchain for presentation
            let image_indices = [image_index];
            let swapchains = [self.swapchain.vk_swapchain];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .image_indices(&image_indices)
                .swapchains(&swapchains);

            self.swapchain
                .swapchain_device
                .queue_present(self.present_queue, &present_info)?;

            // A suboptimal surface is considered a success code and we have acquired an image successfully
            // So recreate it after presenting the image
            if suboptimal_surface {
                log!("Suboptimal surface");
                self.recreate_swapchain()?;
            }
        }

        // Advance the frame, looping back round after every MAX_FRAMES_IN_FLIGHT frames
        self.current_frame = (self.current_frame + 1) % commands::MAX_FRAMES_IN_FLIGHT as usize;

        Ok(())
    }

    /// The window surface can change such that the swapchain is no longer compatible with it (e.g a window resize)
    ///
    /// When these events occur, we should recreate the swapchain so it is compatible with the surface
    pub fn recreate_swapchain(&mut self) -> Result<()>
    {
        log!("Recreating swapchain");

        log!(
            "Window Dimensions: Width {}, Height {}",
            project::WINDOW_WIDTH.get(),
            project::WINDOW_HEIGHT.get()
        );

        // Wait for in process execution to finish first
        unsafe { self.device.device_wait_idle()? };

        // Delete the previous swapchain
        self.swapchain.cleanup(&self.device);

        // Update the surface details with the new surface
        self.surface.details = presentation::get_surface_details(
            self.physical_device.vk_physical_device,
            self.surface.vk_surface,
            &self.surface.loader,
        )?;

        // Create the new swapchain
        self.swapchain = presentation::create_swapchain(&self.instance, &self.device, &self.physical_device, &self.surface)?;
        self.swapchain.create_framebuffers(&self.device, &self.pipeline)?;

        Ok(())
    }
}
