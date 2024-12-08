use crate::graphics::drawing;
use crate::graphics::vk_app::{self, GraphicsError, Result};
use crate::maths::matrix;
use ash::vk;
use std::ffi;

#[repr(C)]
pub struct UniformBufferObject
{
    model: matrix::Matrix4f,
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

pub fn create_vertex_buffer(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
) -> Result<Buffer>
{
    let buffer_size: vk::DeviceSize = size_of_val(&vk_app::VERTICES) as vk::DeviceSize;

    let staging_buffer = create_buffer(
        instance,
        physical_device,
        device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;

    unsafe {
        let data_ptr = device.map_memory(staging_buffer.buffer_memory, 0, buffer_size, vk::MemoryMapFlags::empty())?;
        std::ptr::copy_nonoverlapping(vk_app::VERTICES.as_ptr(), data_ptr.cast(), vk_app::VERTICES.len());
    }

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

    unsafe {
        let data_ptr = device.map_memory(staging_buffer.buffer_memory, 0, buffer_size, vk::MemoryMapFlags::empty())?;
        std::ptr::copy_nonoverlapping(vk_app::INDICES.as_ptr(), data_ptr.cast(), vk_app::INDICES.len());
        device.unmap_memory(staging_buffer.buffer_memory);
    }
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

pub fn create_uniform_buffers(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device,
) -> Result<(Vec<Buffer>, Vec<*mut ffi::c_void>)>
{
    let buffer_size = size_of::<UniformBufferObject>() as vk::DeviceSize;

    let mut uniform_buffers = Vec::<Buffer>::new();
    let mut uniform_buffers_mapped = Vec::<*mut ffi::c_void>::new();

    let usage = vk::BufferUsageFlags::UNIFORM_BUFFER;
    let properties = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    for _ in 0..drawing::MAX_FRAMES_IN_FLIGHT {
        let buffer = create_buffer(instance, physical_device, device, buffer_size, usage, properties)?;

        unsafe {
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

pub fn create_descriptor_pool(device: &ash::Device) -> Result<vk::DescriptorPool>
{
    let pool_sizes: [vk::DescriptorPoolSize; 2] = [
        vk::DescriptorPoolSize {
            ty:               vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: drawing::MAX_FRAMES_IN_FLIGHT,
        },
        vk::DescriptorPoolSize {
            ty:               vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: drawing::MAX_FRAMES_IN_FLIGHT,
        },
    ];

    let pool_create_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(drawing::MAX_FRAMES_IN_FLIGHT);

    Ok(unsafe { device.create_descriptor_pool(&pool_create_info, None) }?)
}

pub fn create_descriptor_sets(
    device: &ash::Device, descriptor_pool: vk::DescriptorPool, uniform_buffers: &Vec<Buffer>,
    descriptor_set_layout: vk::DescriptorSetLayout, texture_image_view: vk::ImageView, texture_sampler: vk::Sampler,
) -> Result<Vec<vk::DescriptorSet>>
{
    let mut layouts = Vec::<vk::DescriptorSetLayout>::new();
    for _ in 0..drawing::MAX_FRAMES_IN_FLIGHT {
        layouts.push(descriptor_set_layout);
    }

    let descriptor_set_allocate_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&layouts);

    let descriptor_sets = unsafe { device.allocate_descriptor_sets(&descriptor_set_allocate_info)? };

    if descriptor_sets.len() != drawing::MAX_FRAMES_IN_FLIGHT as usize
        && uniform_buffers.len() != drawing::MAX_FRAMES_IN_FLIGHT as usize
    {
        // TODO: probably shouldn't be DeviceError
        return Err(GraphicsError::DeviceError(String::from(
            "Descriptor sets and uniform buffers must be same size as MAX_FRAMES_IN_FLIGHT",
        )));
    }

    for (&descriptor_set, uniform_buffer) in descriptor_sets.iter().zip(uniform_buffers.iter()) {
        let buffer_info: [vk::DescriptorBufferInfo; 1] = [vk::DescriptorBufferInfo::default()
            .buffer(uniform_buffer.buffer)
            .offset(0)
            .range(size_of::<UniformBufferObject>() as vk::DeviceSize)];

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

pub(crate) fn create_buffer(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, size: vk::DeviceSize,
    usage: vk::BufferUsageFlags, properties: vk::MemoryPropertyFlags,
) -> Result<Buffer>
{
    let buffer_create_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);

    let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }?;

    let memory_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

    let memory_type = find_memory_type(instance, physical_device, memory_requirements.memory_type_bits, properties)?;

    let memory_allocate_info = vk::MemoryAllocateInfo::default()
        .allocation_size(memory_requirements.size)
        .memory_type_index(memory_type as u32);

    unsafe {
        let device_memory = device.allocate_memory(&memory_allocate_info, None)?;
        device.bind_buffer_memory(buffer, device_memory, 0)?;

        Ok(Buffer { buffer, buffer_memory: device_memory })
    }
}

// TODO: may wish to create a separate command pool for these kinds of short-lived buffers, because the implementation may be able to apply memory allocation optimizations
fn copy_buffer(
    device: &ash::Device, src_buffer: vk::Buffer, dst_buffer: vk::Buffer, size: vk::DeviceSize,
    command_pool: vk::CommandPool, graphics_queue: vk::Queue,
) -> Result<()>
{
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    let copy_region = vk::BufferCopy::default().size(size);
    let copy_regions = [copy_region];
    unsafe { device.cmd_copy_buffer(command_buffer, src_buffer, dst_buffer, &copy_regions) };

    Ok(end_single_time_commands(
        device,
        command_pool,
        command_buffer,
        graphics_queue,
    )?)
}

pub fn begin_single_time_commands(device: &ash::Device, command_pool: vk::CommandPool) -> Result<vk::CommandBuffer>
{
    let allocate_info = vk::CommandBufferAllocateInfo::default()
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_pool(command_pool)
        .command_buffer_count(1);

    let command_buffer = unsafe { device.allocate_command_buffers(&allocate_info) }?[0];

    let command_buffer_begin_info =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

    unsafe { device.begin_command_buffer(command_buffer, &command_buffer_begin_info) }?;

    Ok(command_buffer)
}

pub fn end_single_time_commands(
    device: &ash::Device, command_pool: vk::CommandPool, command_buffer: vk::CommandBuffer, graphics_queue: vk::Queue,
) -> Result<()>
{
    unsafe { device.end_command_buffer(command_buffer) }?;

    let command_buffers = [command_buffer];
    let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);

    let submits = [submit_info];
    unsafe {
        device.queue_submit(graphics_queue, &submits, vk::Fence::null())?;
        device.queue_wait_idle(graphics_queue)?;
        device.free_command_buffers(command_pool, &command_buffers);
    };

    Ok(())
}

pub fn find_memory_type(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, type_filter: u32, properties: vk::MemoryPropertyFlags,
) -> Result<usize>
{
    let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    for (index, memory_type) in memory_properties.memory_types.iter().enumerate() {
        if type_filter & (1 << index) != 0 && (memory_type.property_flags & properties) == properties {
            return Ok(index);
        }
    }
    Err(GraphicsError::DeviceError(String::from(
        "Failed to find suitable memory type",
    )))
}

pub fn update_uniform_buffer(uniform_buffers_mapped: &Vec<*mut ffi::c_void>, current_image: usize)
{
    let model_matrix: matrix::Matrix4f = matrix::SquareMatrix::identity(1.0);
    let ubo = UniformBufferObject { model: model_matrix };
    unsafe {
        std::ptr::copy_nonoverlapping(&ubo, uniform_buffers_mapped[current_image].cast(), 1);
    }
}
