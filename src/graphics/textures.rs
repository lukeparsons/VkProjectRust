use crate::graphics::buffers;
use crate::graphics::vk_app::{GraphicsError, IOResultToResultExt, Result};
use ash::vk;
use std::fs::File;
use std::io;

pub fn create_texture_image(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, command_pool: vk::CommandPool,
    graphics_queue: vk::Queue, path: &str,
) -> Result<(vk::Image, vk::DeviceMemory)>
{
    let decoder = png::Decoder::new(File::open(path).to_result(path)?);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bytes = &buf[..info.buffer_size()];

    if info.color_type != png::ColorType::Rgba {
        return Err(GraphicsError::IoError(
            io::Error::new(io::ErrorKind::InvalidData, "Must be RGBA image"),
            path.to_string(),
        ));
    }

    let image_size = (info.width * info.height * 4) as vk::DeviceSize; // Temp 4

    let usage = vk::BufferUsageFlags::TRANSFER_SRC;
    let properties = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let staging_buffer = buffers::create_buffer(instance, physical_device, device, image_size, usage, properties)?;

    unsafe {
        let data_ptr = device.map_memory(staging_buffer.buffer_memory, 0, image_size, vk::MemoryMapFlags::empty())?;
        std::ptr::copy(bytes.as_ptr() as *mut std::ffi::c_void, data_ptr, image_size as usize);
        device.unmap_memory(staging_buffer.buffer_memory);
    }

    let (texture_image, texture_image_memory) = create_image(instance, physical_device, device, info.width, info.height)?;

    transition_image_layout(
        device,
        command_pool,
        graphics_queue,
        texture_image,
        vk::Format::R8G8B8A8_SRGB,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    )?;

    copy_buffer_to_image(
        device,
        command_pool,
        graphics_queue,
        info.width,
        info.height,
        staging_buffer.buffer,
        texture_image,
    )?;

    transition_image_layout(
        device,
        command_pool,
        graphics_queue,
        texture_image,
        vk::Format::R8G8B8A8_SRGB,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    )?;

    staging_buffer.cleanup(device);

    Ok((texture_image, texture_image_memory))
}

pub fn create_texture_image_view(device: &ash::Device, texture_image: vk::Image) -> Result<vk::ImageView>
{
    let image_view_create_info = vk::ImageViewCreateInfo::default()
        .image(texture_image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(vk::Format::R8G8B8A8_SRGB)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        );

    unsafe { Ok(device.create_image_view(&image_view_create_info, None)?) }
}

pub fn create_texture_sampler(
    instance: &ash::Instance, device: &ash::Device, physical_device: vk::PhysicalDevice,
) -> Result<vk::Sampler>
{
    let properties = unsafe { instance.get_physical_device_properties(physical_device) };

    let sampler_create_info = vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::LINEAR)
        .min_filter(vk::Filter::LINEAR)
        .address_mode_u(vk::SamplerAddressMode::REPEAT)
        .address_mode_v(vk::SamplerAddressMode::REPEAT)
        .address_mode_w(vk::SamplerAddressMode::REPEAT)
        .anisotropy_enable(true)
        .max_anisotropy(properties.limits.max_sampler_anisotropy)
        .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
        .unnormalized_coordinates(false)
        .compare_enable(false)
        .compare_op(vk::CompareOp::ALWAYS)
        .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
        .mip_lod_bias(0.0)
        .min_lod(0.0)
        .max_lod(0.0);

    Ok(unsafe { device.create_sampler(&sampler_create_info, None)? })
}

fn create_image(
    instance: &ash::Instance, physical_device: vk::PhysicalDevice, device: &ash::Device, width: u32, height: u32,
) -> Result<(vk::Image, vk::DeviceMemory)>
{
    let image_create_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .extent(vk::Extent3D { width, height, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .format(vk::Format::R8G8B8A8_SRGB)
        .tiling(vk::ImageTiling::OPTIMAL)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .samples(vk::SampleCountFlags::TYPE_1)
        .flags(vk::ImageCreateFlags::empty());

    let image = unsafe { device.create_image(&image_create_info, None)? };

    let memory_requirements = unsafe { device.get_image_memory_requirements(image) };

    let memory_type = buffers::find_memory_type(
        instance,
        physical_device,
        memory_requirements.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;

    let memory_allocate_info = vk::MemoryAllocateInfo::default()
        .allocation_size(memory_requirements.size)
        .memory_type_index(memory_type as u32);

    unsafe {
        let image_memory = device.allocate_memory(&memory_allocate_info, None)?;
        device.bind_image_memory(image, image_memory, 0)?;

        Ok((image, image_memory))
    }
}

fn transition_image_layout(
    device: &ash::Device, command_pool: vk::CommandPool, graphics_queue: vk::Queue, image: vk::Image, format: vk::Format,
    old_layout: vk::ImageLayout, new_layout: vk::ImageLayout,
) -> Result<()>
{
    let command_buffer = buffers::begin_single_time_commands(device, command_pool)?;

    let mut barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        );

    let (source_stage, destination_stage) = if old_layout == vk::ImageLayout::UNDEFINED
        && new_layout == vk::ImageLayout::TRANSFER_DST_OPTIMAL
    {
        barrier = barrier
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        (vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER)
    } else if old_layout == vk::ImageLayout::TRANSFER_DST_OPTIMAL && new_layout == vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
    {
        barrier = barrier
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        (vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER)
    } else {
        // TODO: probably shouldn't be a device error
        return Err(GraphicsError::DeviceError(String::from("Unsupported layout transition")));
    };

    let buffer_memory_barriers = [barrier];
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            source_stage,
            destination_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &buffer_memory_barriers,
        )
    };

    buffers::end_single_time_commands(device, command_pool, command_buffer, graphics_queue)?;

    Ok(())
}

fn copy_buffer_to_image(
    device: &ash::Device, command_pool: vk::CommandPool, graphics_queue: vk::Queue, width: u32, height: u32,
    buffer: vk::Buffer, image: vk::Image,
) -> Result<()>
{
    let command_buffer = buffers::begin_single_time_commands(device, command_pool)?;

    let region = vk::BufferImageCopy::default()
        .buffer_offset(0)
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(0)
                .base_array_layer(0)
                .layer_count(1),
        )
        .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
        .image_extent(vk::Extent3D { width, height, depth: 1 });

    let regions = [region];
    unsafe {
        device.cmd_copy_buffer_to_image(command_buffer, buffer, image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &regions)
    };

    buffers::end_single_time_commands(device, command_pool, command_buffer, graphics_queue)
}