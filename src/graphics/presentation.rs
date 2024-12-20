use crate::graphics::{device::SupportedPhysicalDevice, errors::VkAppError, pipeline, vk_app::Result};
use crate::{log, project};
use ash::{khr, vk, Device, Entry, Instance};

pub struct Surface
{
    pub vk_surface: vk::SurfaceKHR,
    pub loader:     khr::surface::Instance,
    pub details:    SurfaceDetails,
}

#[derive(Clone)]
pub struct SurfaceDetails
{
    pub capabilities:  vk::SurfaceCapabilitiesKHR,
    pub formats:       Vec<vk::SurfaceFormatKHR>,
    pub present_modes: Vec<vk::PresentModeKHR>,
}

/// A window surface is an abstraction of an OS-specific window. It is the target for our images we wish to be displayed
pub fn create_surface(
    entry: &Entry, instance: &Instance, hwnd: &windows::Win32::Foundation::HWND,
    h_instance: &windows::Win32::Foundation::HINSTANCE,
) -> Result<(khr::surface::Instance, vk::SurfaceKHR)>
{
    let surface_info: vk::Win32SurfaceCreateInfoKHR = vk::Win32SurfaceCreateInfoKHR::default()
        .hwnd(hwnd.0 as isize)
        .hinstance(h_instance.0 as isize);

    let win32_surface_instance = khr::win32_surface::Instance::new(entry, instance);

    let surface_loader = khr::surface::Instance::new(entry, instance);

    let surface = unsafe { win32_surface_instance.create_win32_surface(&surface_info, None) }?;

    Ok((surface_loader, surface))
}

pub fn get_surface_details(
    physical_device: vk::PhysicalDevice, surface: vk::SurfaceKHR, surface_loader: &ash::khr::surface::Instance,
) -> Result<SurfaceDetails>
{
    unsafe {
        let formats = surface_loader.get_physical_device_surface_formats(physical_device, surface)?;
        if formats.len() == 0 {
            return Err(VkAppError::DeviceError(String::from(
                "Device does not have any supported surface formats",
            )));
        }
        let present_modes = surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?;
        if present_modes.len() == 0 {
            return Err(VkAppError::DeviceError(String::from(
                "Device does not have any supported surface present modes",
            )));
        }
        Ok(SurfaceDetails {
            capabilities: surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?,
            formats,
            present_modes,
        })
    }
}

#[derive(Copy, Clone)]
pub struct SwapchainSettings
{
    pub extent:       vk::Extent2D,
    pub format:       vk::SurfaceFormatKHR,
    pub present_mode: vk::PresentModeKHR,
}

pub fn get_swapchain_settings(surface_details: &SurfaceDetails) -> Result<SwapchainSettings>
{
    /*  The swapchain extent is the resolution of swapchain images. It should be the same as the surface extent
       Some window managers let us choose by setting the surface height or width to u32 MAX
       In this case we select the set window resolution (or the minimum/maximum surface extent)
    */
    let mut swapchain_extent = surface_details.capabilities.current_extent;
    if swapchain_extent.height == u32::MAX || swapchain_extent.width == u32::MAX {
        // TODO: Might break on certain displays without 1:1 screen coord to pixel
        swapchain_extent.width = (project::WINDOW_WIDTH.get() as u32).clamp(
            surface_details.capabilities.min_image_extent.width,
            surface_details.capabilities.max_image_extent.width,
        );
        swapchain_extent.height = (project::WINDOW_HEIGHT.get() as u32).clamp(
            surface_details.capabilities.min_image_extent.height,
            surface_details.capabilities.max_image_extent.height,
        );
    }

    // Prefer the SRGB colour format
    let swapchain_format = surface_details
        .formats
        .iter()
        .find(|surface_format| {
            surface_format.format == vk::Format::B8G8R8A8_SRGB
                && surface_format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .or_else(|| surface_details.formats.first())
        .cloned()
        .expect(
            "Device contains no surface formats however get_physical_devices should guarantee it contains at least one!",
        );

    log!(
        "Selected surface format {:?} and colour space {:?}",
        swapchain_format.format,
        swapchain_format.color_space
    );

    // TODO: Make this an option
    let present_mode = match surface_details
        .present_modes
        .iter()
        .find(|&&present_mode| present_mode == vk::PresentModeKHR::MAILBOX)
    {
        Some(present_mode) => {
            println!("Found MAILBOX present mode");
            present_mode
        }
        None => match surface_details.present_modes
            .iter()
            .find(|&&present_mode| present_mode == vk::PresentModeKHR::FIFO) // FIFO should be guaranteed to be available
        {
            Some(present_mode) => {
                log!("Failed to find MAILBOX, using FIFO present mode");
                present_mode
            }
            None => {
                return Err(VkAppError::DeviceError(String::from(
                    "Failed to find acceptable supported present mode",
                )))
            }
        },
    }
    .to_owned();

    Ok(SwapchainSettings {
        extent: swapchain_extent,
        format: swapchain_format,
        present_mode,
    })
}

pub struct Swapchain
{
    pub swapchain_device: khr::swapchain::Device,
    pub vk_swapchain:     vk::SwapchainKHR,
    pub settings:         SwapchainSettings,
    pub image_views:      Vec<vk::ImageView>,
    pub framebuffers:     Vec<vk::Framebuffer>,
}

impl Swapchain
{
    pub fn cleanup(&self, device: &ash::Device)
    {
        unsafe {
            for &swapchain_framebuffer in &self.framebuffers {
                device.destroy_framebuffer(swapchain_framebuffer, None);
            }
            for &swapchain_image_view in &self.image_views {
                device.destroy_image_view(swapchain_image_view, None);
            }
            self.swapchain_device.destroy_swapchain(self.vk_swapchain, None);
        }
    }

    /// The render pass expects a single framebuffer with the same format as the swapchain images
    ///
    /// A vk::Framebuffer object references all the vk::ImageView objects that represent the framebuffer's attachments
    ///
    /// We only have one attachment, the colour attachment so therefore only one ImageView
    ///
    /// However we can retrieve any one of the swapchain images when we present so need to create a framebuffer for all images in the swapchain
    pub fn create_framebuffers(&mut self, device: &ash::Device, pipeline: &pipeline::Pipeline) -> Result<()>
    {
        for &image_view in &self.image_views {
            let attachments: [vk::ImageView; 1] = [image_view];

            let framebuffer_create_info = vk::FramebufferCreateInfo::default()
                .render_pass(pipeline.render_pass)
                .attachments(&attachments)
                .width(self.settings.extent.width)
                .height(self.settings.extent.height)
                .layers(1); // Number of layers in image arrays

            self.framebuffers
                .push(unsafe { device.create_framebuffer(&framebuffer_create_info, None) }?);
        }

        Ok(())
    }
}

/// The swapchain is a queue of images that are waiting to be presented to the screen
///
/// The swapchain synchronizes the presentation of images with the refresh rate of the screen.
pub fn create_swapchain(
    instance: &Instance, device: &Device, physical_device: &SupportedPhysicalDevice, surface: &Surface,
) -> Result<Swapchain>
{
    let swapchain_settings = get_swapchain_settings(&surface.details)?;

    // Select number of images to use in the swapchain
    // Try use one more than the minimum as otherwise we may have to wait for internal driver operations to complete before we can acquire another image to render to
    let mut image_count = surface.details.capabilities.min_image_count + 1;
    // Make sure we don't exceed the maximum image count (max_image_count=0 means no maximum)
    if surface.details.capabilities.max_image_count > 0 && image_count > surface.details.capabilities.max_image_count {
        image_count = surface.details.capabilities.max_image_count;
    }

    let mut swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface.vk_surface)
        .min_image_count(image_count)
        .image_format(swapchain_settings.format.format)
        .image_color_space(swapchain_settings.format.color_space)
        .image_extent(swapchain_settings.extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT) // Render to images directly
        .pre_transform(surface.details.capabilities.current_transform) // No transforms
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE) // Ignore alpha channel for no blending with other windows
        .present_mode(swapchain_settings.present_mode)
        .clipped(true) // Don't care about colour of obscured pixels (e.g when another window is in front of them)
        .old_swapchain(vk::SwapchainKHR::null()) // TODO: will have to modify if swapchain invalidated
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE); // TODO: Ideally always try and use exclusive if graphics and present queue families are the same for best performance

    // Handle swapchain images that are used across multiple queue families (i.e. if graphics queue family is not the same as the presentation queue family)
    let queue_family_indices = [physical_device.graphics_family_index, physical_device.present_family_index];
    if physical_device.graphics_family_index != physical_device.present_family_index {
        swapchain_create_info = swapchain_create_info
            .image_sharing_mode(vk::SharingMode::CONCURRENT)
            .queue_family_indices(&queue_family_indices);
    }

    let swapchain_device = khr::swapchain::Device::new(instance, device);
    let vk_swapchain = unsafe { swapchain_device.create_swapchain(&swapchain_create_info, None) }?;
    let image_views = create_swapchain_image_views(device, &swapchain_device, vk_swapchain, swapchain_settings)?;
    Ok(Swapchain {
        swapchain_device,
        vk_swapchain,
        settings: swapchain_settings,
        image_views,
        framebuffers: Vec::new(),
    })
}

/// An image view is aview into an image (describes how to access the image and which part to access)
///
/// Create a basic image view for every image in the swapchain to use them as targets later
fn create_swapchain_image_views(
    device: &Device, swapchain_device: &khr::swapchain::Device, vk_swapchain: vk::SwapchainKHR,
    swapchain_settings: SwapchainSettings,
) -> Result<Vec<vk::ImageView>>
{
    // Retrieve images stored by swapchain
    let swapchain_images = unsafe { swapchain_device.get_swapchain_images(vk_swapchain) }?;
    let mut swapchain_image_views: Vec<vk::ImageView> = Vec::new();
    for swapchain_image in swapchain_images {
        let image_view_info = vk::ImageViewCreateInfo::default()
            .image(swapchain_image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(swapchain_settings.format.format)
            .components(vk::ComponentMapping {
                r: vk::ComponentSwizzle::IDENTITY,
                g: vk::ComponentSwizzle::IDENTITY,
                b: vk::ComponentSwizzle::IDENTITY,
                a: vk::ComponentSwizzle::IDENTITY,
            })
            // Set the image purpose and which part should be accessed
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask:      vk::ImageAspectFlags::COLOR, // Our images are colour targets
                base_mip_level:   0,
                level_count:      1,
                base_array_layer: 0,
                layer_count:      1,
            });
        swapchain_image_views.push(unsafe { device.create_image_view(&image_view_info, None) }?);
    }

    Ok(swapchain_image_views)
}
