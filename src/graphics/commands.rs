use crate::graphics::presentation::Swapchain;
use crate::graphics::{pipeline, vk_app, vk_app::Result};
use ash::vk;
use ash::vk::ClearColorValue;
/// Allow for multiple frames in flight (rendering of one frame does not interfere with recording of the next)
///
/// 2 stops the CPU getting too far ahead of the GPU
pub const MAX_FRAMES_IN_FLIGHT: u32 = 2;

/*  In Vulkan, operations or 'commands' are added to a device queue like the graphics queue or present queue
   All enqueued commands are submitted together so Vulkan can efficiently process them together
   This also means we can record commands in multiple threads
*/

/// A command pool allocate command buffers and manages the memory used to store them
pub fn create_command_pool(device: &ash::Device, queue_family_index: u32) -> Result<vk::CommandPool>
{
    let command_pool_create_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER) // Command buffers can be rerecorded individually, otherwise they must be all reset together
        .queue_family_index(queue_family_index);

    Ok(unsafe { device.create_command_pool(&command_pool_create_info, None) }?)
}

/// A command buffer is allocated from a command pool and commands are recorded to it to later be submitted to a queue
///
/// Each frame has its own command buffer so we can record a new frame while another is being presented
pub fn create_command_buffers(device: &ash::Device, command_pool: vk::CommandPool) -> Result<Vec<vk::CommandBuffer>>
{
    let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        // PRIMARY means the buffer can be submitted to a queue for execution but cannot be called from other command buffers
        // SECONDARY means the buffer cannot be submitted directly but can be called from primary command buffers
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(MAX_FRAMES_IN_FLIGHT); // Number of comamnd buffers to allocate

    Ok(unsafe { device.allocate_command_buffers(&command_buffer_allocate_info) }?)
}

/// Record commands to begin the render pass, bind the vertex and index buffers and descriptor sets, set the dynamic states of the pipeline and lastly issue the draw commands
pub fn record_command_buffer(
    device: &ash::Device, command_buffer: vk::CommandBuffer, image_index: u32, pipeline: &pipeline::Pipeline,
    swapchain: &Swapchain, vertex_buffer: vk::Buffer, index_buffer: vk::Buffer,
    descriptor_sets_current_frame: Vec<vk::DescriptorSet>,
) -> Result<()>
{
    let command_buffer_begin_info = vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::empty());

    unsafe { device.begin_command_buffer(command_buffer, &command_buffer_begin_info) }?;

    // We are using SRGB which is floating point so must floating point for our clear values
    // TODO: Make compatible with other formats
    let clear_colour = vk::ClearValue { color: ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } };
    let clear_values: [vk::ClearValue; 1] = [clear_colour];

    let render_pass_begin_info = vk::RenderPassBeginInfo::default()
        .render_pass(pipeline.render_pass)
        .framebuffer(swapchain.framebuffers[image_index as usize])
        // Render area determines where the shader loads and stores take place
        .render_area(vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: swapchain.settings.extent,
        })
        .clear_values(&clear_values);

    unsafe {
        // INLINE SubpassContents means the render pass commands are embedded in the primary command buffer itself and no secondary command buffers are executed
        device.cmd_begin_render_pass(command_buffer, &render_pass_begin_info, vk::SubpassContents::INLINE);
        device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::GRAPHICS, pipeline.graphics_pipeline);

        device.cmd_bind_vertex_buffers(command_buffer, 0, &[vertex_buffer], &[0]);

        // Need to
        device.cmd_bind_descriptor_sets(
            command_buffer,
            vk::PipelineBindPoint::GRAPHICS, // Must specify pipeline as descriptor sets are not unique to graphics pipelines
            pipeline.pipeline_layout,
            0,
            &descriptor_sets_current_frame,
            &[],
        );

        device.cmd_bind_index_buffer(command_buffer, index_buffer, 0, vk::IndexType::UINT16);
    }

    // Viewport and scissor state for the pipeline are dynamic so need to set them in command buffer before submitting draw command
    let viewport = vk::Viewport::default()
        .x(0.0)
        .y(0.0)
        .width(swapchain.settings.extent.width as f32)
        .height(swapchain.settings.extent.height as f32)
        .min_depth(0.0)
        .max_depth(0.0);

    unsafe { device.cmd_set_viewport(command_buffer, 0, [viewport].as_slice()) };

    let scissor = vk::Rect2D::default()
        .offset(vk::Offset2D { x: 0, y: 0 })
        .extent(swapchain.settings.extent);

    unsafe {
        device.cmd_set_scissor(command_buffer, 0, [scissor].as_slice());
        device.cmd_draw_indexed(command_buffer, vk_app::INDICES.len() as u32, 1, 0, 0, 0);
        device.cmd_end_render_pass(command_buffer);
        Ok(device.end_command_buffer(command_buffer)?)
    }
}

/// Semaphores in this struct are used for synchronising swapchain operations which happen on the GPU
///
/// Fences in this struct are used for the host to wait until the previous frame has finished rendering. This prevents drawing more than one frame at a time
///
/// Each frame has its own set of semaphores and fence
pub struct SyncObjects
{
    pub image_available_semaphores: Vec<vk::Semaphore>,
    pub render_finished_semaphores: Vec<vk::Semaphore>,
    pub in_flight_fences:           Vec<vk::Fence>,
}

impl SyncObjects
{
    pub fn cleanup(&self, device: &ash::Device)
    {
        unsafe {
            for &semaphore in &self.image_available_semaphores {
                device.destroy_semaphore(semaphore, None);
            }
            for &semaphore in &self.render_finished_semaphores {
                device.destroy_semaphore(semaphore, None);
            }
            for &fence in &self.in_flight_fences {
                device.destroy_fence(fence, None);
            }
        }
    }
}

/// Many Vulkan API calls that start executing work on the GPU are asynchronous, the functions return before the operation completes
///
/// Therefore, we need to explicitly synchronise execution of operations on the GPU
///
/// We use semaphores to add order between queue operations (work we submit to a queue from a command buffer or within a function)
///
/// We use fences to order execution on the CPU (the host). A fence alerts the host when the GPU has finished some execution. Fences block host execution whereas semaphores do not
pub fn create_sync_objects(device: &ash::Device) -> Result<SyncObjects>
{
    let semaphore_create_info = vk::SemaphoreCreateInfo::default();

    // Start fence as signaled to stop indefinite block on first frame as there are no previous frames to signal the fence
    let fence_create_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

    let mut image_available_semaphores = Vec::<vk::Semaphore>::new();
    let mut render_finished_semaphores = Vec::<vk::Semaphore>::new();
    let mut in_flight_fences = Vec::<vk::Fence>::new();

    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        unsafe {
            image_available_semaphores.push(device.create_semaphore(&semaphore_create_info, None)?);
            render_finished_semaphores.push(device.create_semaphore(&semaphore_create_info, None)?);
            in_flight_fences.push(device.create_fence(&fence_create_info, None)?);
        }
    }
    Ok(SyncObjects {
        image_available_semaphores,
        render_finished_semaphores,
        in_flight_fences,
    })
}
