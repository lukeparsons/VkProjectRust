use crate::graphics::commands;
use crate::graphics::commands::MAX_FRAMES_IN_FLIGHT;
use crate::graphics::errors::VkAppError;
use crate::graphics::vk_app::{self, Result};
use crate::maths::{matrix, vector};
use ash::vk;
use std::ffi;

#[repr(C, align(16))]
pub struct Aligned16<T>(T);

#[repr(C)]
pub struct UniformBufferObject
{
    model:      Aligned16<matrix::Matrix4f>,
    projection: Aligned16<matrix::Matrix4f>,
}

pub struct Buffer
{
    pub buffer:        vk::Buffer,
    pub buffer_memory: vk::DeviceMemory,
}

impl Buffer
{
    pub fn cleanup(&self, device: &ash::Device)
    {
        unsafe {
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.buffer_memory, None);
        }
    }
}

/// Copy data into a buffer allocated from the GPU
// TODO: Size check by abstracting buffer_memory?
unsafe fn buffer_memcpy<T>(device: &ash::Device, buffer_memory: vk::DeviceMemory, src_data: &[T]) -> Result<()>
{
    let data_ptr = device.map_memory(
        buffer_memory,
        0,
        size_of_val(src_data) as vk::DeviceSize,
        vk::MemoryMapFlags::empty(),
    )?;
    std::ptr::copy_nonoverlapping(src_data.as_ptr(), data_ptr.cast(), src_data.len());
    device.unmap_memory(buffer_memory);
    Ok(())
}

pub fn create_vertex_buffer(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
) -> Result<Buffer>
{
    let buffer_size: vk::DeviceSize = size_of_val(&vk_app::VERTICES) as vk::DeviceSize;

    /*  The most optimal memory for the GPU to read from has the VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT flag
       This memory is usually not accessible by the CPU on dedicated graphics cards
       The staging buffer can be accessed by the CPU which data is uploaded to
       The staging buffer then uploads the data to device local memory
    */
    let staging_buffer = create_buffer(
        instance,
        physical_device,
        device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        /*  HOST_VISIBLE lets us map the memory so we can write to it from the CPU
           HOST_COHERENT ensures the mapped memory always matches the contents of the allocated memory
           Useful because driver may not immediately copy data into buffer memory
        */
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;

    // Copy our vertices into the memory we have just allocated and bound to the vertex buffer
    // This memcpy is only guaranteed to be complete once we submit the queue of commands
    unsafe { buffer_memcpy(device, staging_buffer.buffer_memory, &vk_app::VERTICES) }?;

    let usage = vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER;
    let properties = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    let vertex_buffer = create_buffer(instance, physical_device, device, buffer_size, usage, properties)?;

    copy_buffer(
        device,
        staging_buffer.buffer,
        vertex_buffer.buffer,
        buffer_size,
        command_pool,
        graphics_queue,
    )?;

    staging_buffer.cleanup(device);

    Ok(vertex_buffer)
}

pub fn create_index_buffer(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
) -> Result<Buffer>
{
    let buffer_size: vk::DeviceSize = size_of_val(&vk_app::INDICES) as vk::DeviceSize;
    let usage = vk::BufferUsageFlags::TRANSFER_SRC;
    let properties = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let staging_buffer = create_buffer(instance, physical_device, device, buffer_size, usage, properties)?;

    unsafe { buffer_memcpy(device, staging_buffer.buffer_memory, &vk_app::INDICES) }?;

    let usage = vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER;
    let properties = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    let index_buffer = create_buffer(instance, physical_device, device, buffer_size, usage, properties)?;

    copy_buffer(
        device,
        staging_buffer.buffer,
        index_buffer.buffer,
        buffer_size,
        command_pool,
        graphics_queue,
    )?;

    staging_buffer.cleanup(device);

    Ok(index_buffer)
}

/// Allocate a uniform buffers for each frame
pub fn create_uniform_buffers(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device,
) -> Result<(Vec<Buffer>, Vec<*mut ffi::c_void>)>
{
    // No need to use a staging buffer because we will copy new data to the uniform buffer every frame
    // Would just add extra overhead which could degrade performance
    // TODO: Could use staging buffer for uniform values unlikely to change often? e.g world position
    let buffer_size = size_of::<UniformBufferObject>() as vk::DeviceSize;

    // We create multiple buffers because multiple frames may be in flight at the same time
    // We don't want to update the buffer in preparation of the next frame while a previous one is still reading from it
    let mut uniform_buffers = Vec::<Buffer>::new(); // TODO: Reserve/resize?
    let mut uniform_buffers_mapped = Vec::<*mut ffi::c_void>::new();

    let usage = vk::BufferUsageFlags::UNIFORM_BUFFER;
    let properties = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    for _ in 0..commands::MAX_FRAMES_IN_FLIGHT {
        let buffer = create_buffer(instance, physical_device, device, buffer_size, usage, properties)?;

        unsafe {
            // The buffer stays mapped for the application's whole lifetime which increases performance as we don't need to re-map every frame
            uniform_buffers_mapped.push(device.map_memory(
                buffer.buffer_memory,
                0,
                buffer_size,
                vk::MemoryMapFlags::empty(),
            )?)
        };

        uniform_buffers.push(buffer);
    }

    Ok((uniform_buffers, uniform_buffers_mapped))
}

/// Descriptor sets must be allocated from a descriptor pol
pub fn create_descriptor_pool(device: &ash::Device) -> Result<vk::DescriptorPool>
{
    // The types of descriptor sets and number of them we will create
    let pool_sizes: [vk::DescriptorPoolSize; 2] = [
        vk::DescriptorPoolSize {
            ty:               vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: commands::MAX_FRAMES_IN_FLIGHT,
        },
        vk::DescriptorPoolSize {
            ty:               vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: commands::MAX_FRAMES_IN_FLIGHT,
        },
    ];

    // An additional flag exists for freeing individual descriptor sets, if that's ever needed
    let pool_create_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(commands::MAX_FRAMES_IN_FLIGHT);

    Ok(unsafe { device.create_descriptor_pool(&pool_create_info, None) }?)
}

/// A descriptor set specifies the buffer/image resources that are bound to descriptors which are used by shaders
///
/// The descriptor set is bound for the drawing commands just like the vertex and index buffer and framebuffer
///
/// Creates one descriptor set per frame
pub fn create_descriptor_sets(
    device: &ash::Device, descriptor_pool: vk::DescriptorPool, uniform_buffers: &Vec<Buffer>,
    descriptor_set_layout: vk::DescriptorSetLayout, texture_image_view: vk::ImageView, texture_sampler: vk::Sampler,
) -> Result<Vec<vk::DescriptorSet>>
{
    let layouts = vec![descriptor_set_layout; MAX_FRAMES_IN_FLIGHT as usize];

    let descriptor_set_allocate_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&layouts);

    let descriptor_sets = unsafe { device.allocate_descriptor_sets(&descriptor_set_allocate_info)? };

    if descriptor_sets.len() != MAX_FRAMES_IN_FLIGHT as usize && uniform_buffers.len() != MAX_FRAMES_IN_FLIGHT as usize {
        // TODO: probably shouldn't be DeviceError
        return Err(VkAppError::DeviceError(String::from(
            "Descriptor sets and uniform buffers must be same size as MAX_FRAMES_IN_FLIGHT",
        )));
    }

    // Configure descriptors in descriptor sets
    // TODO: Shouldn't be zipped with uniform buffers when we also do image_info
    for (&descriptor_set, uniform_buffer) in descriptor_sets.iter().zip(uniform_buffers.iter()) {
        let buffer_info: [vk::DescriptorBufferInfo; 1] = [vk::DescriptorBufferInfo::default()
            .buffer(uniform_buffer.buffer)
            .offset(0)
            .range(size_of::<UniformBufferObject>() as vk::DeviceSize)]; // TODO: Can use VK_WHOLE_SIZE for range if overwriting whole buffer

        let image_info: [vk::DescriptorImageInfo; 1] = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(texture_image_view)
            .sampler(texture_sampler)];

        let descriptor_writes: [vk::WriteDescriptorSet; 2] = [
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .buffer_info(&buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .image_info(&image_info),
        ];

        unsafe { device.update_descriptor_sets(&descriptor_writes, &[]) };
    }

    Ok(descriptor_sets)
}

/// Allocate GPU memory then bind the buffer to it
pub(crate) fn create_buffer(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, size: vk::DeviceSize,
    usage: vk::BufferUsageFlags, properties: vk::MemoryPropertyFlags,
) -> Result<Buffer>
{
    let buffer_create_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE); // Only used from graphics queue so exclusive access

    let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }?;

    // Get requirements for the buffer
    let memory_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

    // Find the correct memory type for the buffer using its requirements and the requested properties
    let memory_type = find_memory_type(instance, physical_device, memory_requirements.memory_type_bits, properties)?;

    let memory_allocate_info = vk::MemoryAllocateInfo::default()
        .allocation_size(memory_requirements.size)
        .memory_type_index(memory_type as u32);

    unsafe {
        // TODO: Should not be calling allocate_memory for every individual buffer as number of simulatenous is limited by device which can be very low
        // Instead should make one allocation for many objects and use offset parameters
        let device_memory = device.allocate_memory(&memory_allocate_info, None)?;
        // Associate the buffer with the allocated memory
        device.bind_buffer_memory(buffer, device_memory, 0)?;

        Ok(Buffer { buffer, buffer_memory: device_memory })
    }
}

/// Copy one buffer to another
///
/// Typically copying a staging buffer to a device local one
fn copy_buffer(
    device: &ash::Device, src_buffer: vk::Buffer, dst_buffer: vk::Buffer, size: vk::DeviceSize,
    command_pool: vk::CommandPool, graphics_queue: vk::Queue,
) -> Result<()>
{
    // Memory transfer operations are executed using command buffers so must allocate a temporary command buffer
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    let copy_region = vk::BufferCopy::default().size(size);
    unsafe { device.cmd_copy_buffer(command_buffer, src_buffer, dst_buffer, &[copy_region]) };

    Ok(end_single_time_commands(
        device,
        command_pool,
        command_buffer,
        graphics_queue,
    )?)
}

/// Create a temporary command buffer and set the command buffer to immediately start recording and submit once
pub fn begin_single_time_commands(device: &ash::Device, command_pool: vk::CommandPool) -> Result<vk::CommandBuffer>
{
    // TODO: may wish to create a separate command pool for these kinds of short-lived buffers, because the implementation may be able to apply memory allocation optimizations
    let allocate_info = vk::CommandBufferAllocateInfo::default()
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_pool(command_pool)
        .command_buffer_count(1);

    let command_buffer = unsafe { device.allocate_command_buffers(&allocate_info) }?[0];

    // ONE_TIME_SUBMIT indicates to the driver that we will use the command buffer once and wait until its commands are finished
    let command_buffer_begin_info =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

    // Begin recording
    unsafe { device.begin_command_buffer(command_buffer, &command_buffer_begin_info) }?;

    Ok(command_buffer)
}

/// Submit the temporary, one time submit command buffer and wait until its complete
///
/// Currently using graphics_queue as both either graphics queue and present queue support buffer transfer operations
// TODO: Can support multiple simulatenous transfers using a fence
pub fn end_single_time_commands(
    device: &ash::Device, command_pool: vk::CommandPool, command_buffer: vk::CommandBuffer, graphics_queue: vk::Queue,
) -> Result<()>
{
    unsafe { device.end_command_buffer(command_buffer) }?;

    let command_buffers = [command_buffer];
    let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);

    unsafe {
        device.queue_submit(graphics_queue, &[submit_info], vk::Fence::null())?;
        // Wait for the queue being used for transfer to become idle
        device.queue_wait_idle(graphics_queue)?;
        device.free_command_buffers(command_pool, &command_buffers);
    };

    Ok(())
}

/// Graphics cards have different types of memory to allocate from
///
/// Each type varies in allowed operations and performance
pub fn find_memory_type(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, type_filter: u32, properties: vk::MemoryPropertyFlags,
) -> Result<usize>
{
    // memory_properties contains the memory heaps from which GPU memory can be allocated (e.g dedicated VRAM, swap space in RAM)
    let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    for i in 0..memory_properties.memory_type_count as usize {
        // Check if memory type is suitable for vertex buffer and has the properties we want
        if type_filter & (1 << i) != 0 && (memory_properties.memory_types[i].property_flags & properties) == properties {
            return Ok(i);
        }
    }
    Err(VkAppError::DeviceError(String::from("Failed to find suitable memory type")))
}

pub fn update_uniform_buffer(uniform_buffers_mapped: &Vec<*mut ffi::c_void>, current_image: usize)
{
    let model_matrix = matrix::Matrix4f::translation_matrix(vector::Vector3f::new([0.0, 0.0, 5.0]));
    let projection_matrix = matrix::Matrix4f::projection_matrix(60.0, 60.0, 0.0);
    let ubo = UniformBufferObject {
        model:      Aligned16::<matrix::Matrix4f>(model_matrix),
        projection: Aligned16::<matrix::Matrix4f>(projection_matrix),
    };
    unsafe {
        std::ptr::copy_nonoverlapping(&ubo, uniform_buffers_mapped[current_image].cast(), 1);
    }
}
