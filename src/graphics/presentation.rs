use crate::graphics::device::{SupportedPhysicalDevice, VkError};
use crate::project;
use ash::{khr, vk, Device, Entry, Instance};

pub struct Surface
{
    pub vk_surface: vk::SurfaceKHR,
    pub loader:     khr::surface::Instance,
    pub settings:   SurfaceSettings,
}

pub fn create_surface(
    entry: &Entry, instance: &Instance, hwnd: &windows::Win32::Foundation::HWND,
    h_instance: &windows::Win32::Foundation::HINSTANCE,
) -> Result<(khr::surface::Instance, vk::SurfaceKHR), VkError>
{
    let surface_info: vk::Win32SurfaceCreateInfoKHR = vk::Win32SurfaceCreateInfoKHR::default()
        .hwnd(hwnd.0 as isize)
        .hinstance(h_instance.0 as isize);

    let win32_surface_instance = khr::win32_surface::Instance::new(entry, instance);

    let surface = unsafe { win32_surface_instance.create_win32_surface(&surface_info, None) }?;

    let surface_loader = khr::surface::Instance::new(entry, instance);

    Ok((surface_loader, surface))
}

#[derive(Copy, Clone)]
pub struct SurfaceSettings
{
    pub capabilities: vk::SurfaceCapabilitiesKHR,
    pub extent:       vk::Extent2D,
    pub format:       vk::SurfaceFormatKHR,
    pub present_mode: vk::PresentModeKHR,
}

pub fn get_surface_settings(
    physical_device: vk::PhysicalDevice, surface_loader: &khr::surface::Instance, surface: vk::SurfaceKHR,
) -> Result<SurfaceSettings, VkError>
{
    let surface_capabilities = unsafe { surface_loader.get_physical_device_surface_capabilities(physical_device, surface) }?;
    let mut surface_extent = surface_capabilities.current_extent;
    if surface_extent.height == u32::MAX || surface_extent.width == u32::MAX {
        // TODO: Might break on certain displays without 1:1 screen coord to pixel
        surface_extent.width = project::WINDOW_WIDTH as u32;
        surface_extent.height = project::WINDOW_HEIGHT as u32;
    }

    let surface_format = unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface) }?
        .iter()
        .find(|surface_format| {
            surface_format.format == vk::Format::B8G8R8A8_SRGB
                && surface_format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .ok_or("Device does not support correct surface format")?
        .to_owned();

    let supported_present_modes =
        unsafe { surface_loader.get_physical_device_surface_present_modes(physical_device, surface) }?;

    // TODO: Make this an option
    let present_mode = match supported_present_modes
        .iter()
        .find(|&&present_mode| present_mode == vk::PresentModeKHR::MAILBOX)
    {
        Some(present_mode) => {
            println!("Found MAILBOX present mode");
            present_mode
        }
        None => match supported_present_modes
            .iter()
            .find(|&&present_mode| present_mode == vk::PresentModeKHR::FIFO)
        {
            Some(present_mode) => {
                println!("Failed to find MAILBOX, using FIFO present mode");
                present_mode
            }
            None => return Err("Failed to find acceptable supported present mode".into()),
        },
    }
    .to_owned();

    Ok(SurfaceSettings {
        capabilities: surface_capabilities,
        extent: surface_extent,
        format: surface_format,
        present_mode,
    })
}

pub struct Swapchain
{
    pub swapchain_device: khr::swapchain::Device,
    pub vk_swapchain:     vk::SwapchainKHR,
    pub image_views:      Vec<vk::ImageView>,
}

pub fn create_swapchain(
    instance: &Instance, device: &Device, physical_device: &SupportedPhysicalDevice, surface: &Surface,
) -> Result<Swapchain, VkError>
{
    let mut image_count = surface.settings.capabilities.min_image_count + 1;
    if surface.settings.capabilities.max_image_count > 0 && image_count > surface.settings.capabilities.max_image_count {
        image_count = surface.settings.capabilities.max_image_count;
    }

    let mut swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface.vk_surface)
        .min_image_count(image_count)
        .image_format(surface.settings.format.format)
        .image_color_space(surface.settings.format.color_space)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .pre_transform(surface.settings.capabilities.current_transform) // No transforms
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE) // Ignore alpha channel (TODO: problem?)
        .present_mode(surface.settings.present_mode)
        .clipped(true)
        .image_extent(surface.settings.extent);

    let queue_family_indices = [physical_device.graphics_family_index, physical_device.present_family_index];
    if physical_device.graphics_family_index != physical_device.present_family_index {
        // TODO: Always use this for best performance
        swapchain_create_info = swapchain_create_info
            .image_sharing_mode(vk::SharingMode::CONCURRENT)
            .queue_family_indices(&queue_family_indices);
    }

    let swapchain_device = khr::swapchain::Device::new(instance, device);
    let vk_swapchain = unsafe { swapchain_device.create_swapchain(&swapchain_create_info, None) }?;
    let image_views = create_swapchain_image_views(device, &swapchain_device, vk_swapchain, &surface.settings)?;
    Ok(Swapchain { swapchain_device, vk_swapchain, image_views })
}

fn create_swapchain_image_views(
    device: &Device, swapchain_device: &khr::swapchain::Device, vk_swapchain: vk::SwapchainKHR,
    surface_settings: &SurfaceSettings,
) -> Result<Vec<vk::ImageView>, VkError>
{
    let swapchain_images = unsafe { swapchain_device.get_swapchain_images(vk_swapchain) }?;
    let mut swapchain_image_views: Vec<vk::ImageView> = Vec::new();
    for swapchain_image in swapchain_images {
        let image_view_info = vk::ImageViewCreateInfo::default()
            .image(swapchain_image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(surface_settings.format.format)
            .components(vk::ComponentMapping {
                r: vk::ComponentSwizzle::IDENTITY,
                g: vk::ComponentSwizzle::IDENTITY,
                b: vk::ComponentSwizzle::IDENTITY,
                a: vk::ComponentSwizzle::IDENTITY,
            })
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask:      vk::ImageAspectFlags::COLOR,
                base_mip_level:   0,
                level_count:      1,
                base_array_layer: 0,
                layer_count:      1,
            });
        swapchain_image_views.push(unsafe { device.create_image_view(&image_view_info, None) }?);
    }

    Ok(swapchain_image_views)
}
