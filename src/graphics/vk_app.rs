use crate::graphics::buffers::{create_descriptor_pool, create_descriptor_sets};
use crate::graphics::presentation::create_swapchain;
use crate::graphics::{buffers, device, drawing, pipeline, presentation, textures};
use ash::ext::debug_utils;
use ash::{vk, Entry, Instance};
use std::error::Error;
use std::{ffi, fmt, io};

#[repr(C)]
pub struct Vertex
{
    pub position:  [f32; 2],
    pub colour:    [f32; 3],
    pub tex_coord: [f32; 2],
}

pub const VERTICES: [Vertex; 4] = [
    Vertex {
        position:  [-0.5, -0.5],
        colour:    [1.0, 0.0, 0.0],
        tex_coord: [1.0, 0.0],
    },
    Vertex {
        position:  [0.5, -0.5],
        colour:    [0.0, 1.0, 0.0],
        tex_coord: [0.0, 0.0],
    },
    Vertex {
        position:  [0.5, 0.5],
        colour:    [0.0, 0.0, 1.0],
        tex_coord: [0.0, 1.0],
    },
    Vertex {
        position:  [-0.5, 0.5],
        colour:    [1.0, 1.0, 1.0],
        tex_coord: [1.0, 1.0],
    },
];

pub const INDICES: [u16; 6] = [0, 1, 2, 2, 3, 0];

pub type Result<T> = std::result::Result<T, GraphicsError>;

#[derive(Debug)]
pub enum GraphicsError
{
    VkError(vk::Result),
    IoError(std::io::Error, String),
    DeviceError(String),
}

impl fmt::Display for GraphicsError
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        match *self {
            GraphicsError::VkError(ref err) => {
                f.write_fmt(format_args!("Vulkan Error, Code {}: {}", err.as_raw(), err.to_string()))
            }
            GraphicsError::IoError(ref err, ref file) => {
                f.write_fmt(format_args!("IO Error: {} for file {}", err.to_string(), file))
            }
            GraphicsError::DeviceError(ref str) => f.write_fmt(format_args!("Device Error: {}", str)),
        }
    }
}

impl Error for GraphicsError
{
    fn source(&self) -> Option<&(dyn Error + 'static)>
    {
        match *self {
            GraphicsError::VkError(ref err) => Some(err),
            GraphicsError::IoError(ref err, _) => Some(err),
            GraphicsError::DeviceError(_) => None,
        }
    }
}

impl From<vk::Result> for GraphicsError
{
    fn from(vk_result: vk::Result) -> Self
    {
        let vk_error = vk_result
            .result()
            .expect_err("Trying to unwrap successful Vulkan Result as error");
        GraphicsError::VkError(vk_error)
    }
}

pub trait IOResultToResultExt<T>
{
    fn to_result(self, path: &str) -> Result<T>;
}

impl<T> IOResultToResultExt<T> for io::Result<T>
{
    fn to_result(self, path: &str) -> Result<T> { self.map_err(|err| GraphicsError::IoError(err, path.to_string())) }
}

pub struct VkApp
{
    _entry:                 Entry, // For loading vulkan, must have same lifetime as struct
    instance:               Instance,
    debug_utils_loader:     debug_utils::Instance,
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
    uniform_buffers_mapped: Vec<*mut ffi::c_void>,
    descriptor_pool:        vk::DescriptorPool,
    descriptor_sets:        Vec<vk::DescriptorSet>,
    command_buffers:        Vec<vk::CommandBuffer>,
    sync_objects:           drawing::SyncObjects,
    current_frame:          usize,
}

impl Drop for VkApp
{
    fn drop(&mut self)
    {
        println!("Cleaning up");
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
    }
}

impl VkApp
{
    pub fn new(hwnd: &windows::Win32::Foundation::HWND, h_instance: &windows::Win32::Foundation::HINSTANCE) -> Result<Self>
    {
        //let entry = unsafe { Entry::load().unwrap() };
        let entry = Entry::linked(); // Dev only
        let instance = device::create_instance(&entry)?;
        let (debug_utils_loader, debug_callback) = device::create_debug_messenger(&entry, &instance)?;
        let (surface_loader, vk_surface) = presentation::create_surface(&entry, &instance, hwnd, h_instance)?;
        // Just get the first device
        let (physical_device, surface_settings) =
            match device::get_physical_devices(&instance, &surface_loader, vk_surface)?.get(0) {
                Some((physical_device, surface_settings)) => {
                    println!("Selected device {}", physical_device.device_name);
                    (physical_device.to_owned(), surface_settings.to_owned())
                }
                None => return Err(GraphicsError::DeviceError(String::from("No supported devices"))),
            };
        let device = device::create_logical_device(&instance, &physical_device)?;

        let surface = presentation::Surface {
            loader: surface_loader,
            vk_surface,
            settings: surface_settings,
        };

        let (graphics_queue, present_queue) = unsafe {
            (
                device.get_device_queue(physical_device.graphics_family_index, 0),
                device.get_device_queue(physical_device.present_family_index, 0),
            )
        };

        let mut swapchain = presentation::create_swapchain(&instance, &device, &physical_device, &surface)?;
        // TODO: move descriptor_set_layout out of pipeline
        let pipeline = pipeline::create_pipeline(&device, surface_settings, &swapchain)?;
        swapchain.create_framebuffers(&device, &pipeline)?;

        let command_pool = drawing::create_command_pool(&device, physical_device.graphics_family_index)?;

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

        let descriptor_pool = create_descriptor_pool(&device)?;

        let descriptor_sets = create_descriptor_sets(
            &device,
            descriptor_pool,
            &uniform_buffers,
            pipeline.descriptor_set_layout,
            texture_image_view,
            texture_sampler,
        )?;

        let command_buffers = drawing::create_command_buffers(&device, command_pool)?;

        let sync_objects = drawing::create_sync_objects(&device)?;

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
            // Wait until previous frame has finished
            self.device
                .wait_for_fences(&[self.sync_objects.in_flight_fences[self.current_frame]], true, u64::MAX)?;

            let (image_index, suboptimal_surface) = match self.swapchain.swapchain_device.acquire_next_image(
                self.swapchain.vk_swapchain,
                u64::MAX,
                self.sync_objects.image_available_semaphores[self.current_frame],
                vk::Fence::null(),
            ) {
                Ok((image_index, suboptimal_surface)) => (image_index, suboptimal_surface),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate_swapchain()?;
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            };

            if suboptimal_surface {
                eprintln!("Warning: surface is suboptimal for the surface");
            }

            buffers::update_uniform_buffer(&self.uniform_buffers_mapped, self.current_frame);

            self.device
                .reset_fences(&[self.sync_objects.in_flight_fences[self.current_frame]])?;

            self.device
                .reset_command_buffer(self.command_buffers[self.current_frame], vk::CommandBufferResetFlags::empty())?;

            drawing::record_command_buffer(
                &self.device,
                self.command_buffers[self.current_frame],
                image_index,
                &self.pipeline,
                &self.swapchain,
                self.vertex_buffer.buffer,
                self.index_buffer.buffer,
                vec![self.descriptor_sets[self.current_frame]],
            )?;

            let wait_semaphores: [vk::Semaphore; 1] = [self.sync_objects.image_available_semaphores[self.current_frame]];
            let wait_stages: [vk::PipelineStageFlags; 1] = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
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

            let image_indices = [image_index];
            let swapchains = [self.swapchain.vk_swapchain];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .image_indices(&image_indices)
                .swapchains(&swapchains);

            self.swapchain
                .swapchain_device
                .queue_present(self.present_queue, &present_info)?;
        }

        self.current_frame = (self.current_frame + 1) % drawing::MAX_FRAMES_IN_FLIGHT as usize;

        Ok(())
    }

    pub fn recreate_swapchain(&mut self) -> Result<()>
    {
        println!("Recreating swapchain");

        unsafe { self.device.device_wait_idle()? };

        self.surface.settings = presentation::get_surface_settings(
            self.physical_device.vk_physical_device,
            &self.surface.loader,
            self.surface.vk_surface,
        )?;

        self.swapchain.cleanup(&self.device);

        self.swapchain = create_swapchain(&self.instance, &self.device, &self.physical_device, &self.surface)?;

        Ok(())
    }
}
