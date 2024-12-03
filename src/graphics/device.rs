use crate::graphics::presentation::SurfaceSettings;
use crate::graphics::{extensions::*, presentation};
use crate::project;
use ash::{ext::debug_utils, khr, vk, Entry, Instance};
use derive_more::Display;
use std::borrow::Cow;
use std::collections::HashSet;
use std::error::Error;
use std::ffi::{CStr, CString};
use windows;

#[derive(Debug, Display)]
pub struct VkError(String);

impl Error for VkError {}

impl From<vk::Result> for VkError
{
    fn from(vk_result: vk::Result) -> Self
    {
        let vk_error = vk_result
            .result()
            .expect_err("Trying to unwrap successful Vulkan Result as error");
        VkError(format!("Code {}: {}", vk_error.as_raw().to_string(), vk_error.to_string()))
    }
}

impl From<String> for VkError
{
    fn from(value: String) -> Self { VkError(value) }
}

impl From<&str> for VkError
{
    fn from(value: &str) -> Self { VkError(value.to_string()) }
}

const VALIDATION_LAYERS: Extensions<1> = Extensions([c"VK_LAYER_KHRONOS_validation"]);
const EXTENSIONS: Extensions<3> = Extensions([vk::KHR_SURFACE_NAME, vk::EXT_DEBUG_UTILS_NAME, vk::KHR_WIN32_SURFACE_NAME]);
const DEVICE_EXTENSIONS: Extensions<1> = Extensions([vk::KHR_SWAPCHAIN_NAME]);

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT, message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>, _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32
{
    let callback_data = *p_callback_data;
    let message_id_number = callback_data.message_id_number;

    let message_id_name = if callback_data.p_message_id_name.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    println!("{message_severity:?}:\n{message_type:?} [{message_id_name} ({message_id_number})] : {message}\n",);

    vk::FALSE
}

pub struct VkApp
{
    _entry:             Entry, // For loading vulkan, must have same lifetime as struct
    instance:           Instance,
    debug_utils_loader: debug_utils::Instance,
    debug_callback:     vk::DebugUtilsMessengerEXT,
    physical_device:    SupportedPhysicalDevice,
    surface:            presentation::Surface,
    device:             ash::Device,
    graphics_queue:     vk::Queue,
    present_queue:      vk::Queue,
    swapchain:          presentation::Swapchain,
}

impl Drop for VkApp
{
    fn drop(&mut self)
    {
        println!("Cleaning up");
        unsafe {
            for &swapchain_image_view in &self.swapchain.image_views {
                self.device.destroy_image_view(swapchain_image_view, None);
            }
            self.swapchain
                .swapchain_device
                .destroy_swapchain(self.swapchain.vk_swapchain, None);
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
    pub fn new(
        hwnd: &windows::Win32::Foundation::HWND, h_instance: &windows::Win32::Foundation::HINSTANCE,
    ) -> Result<Self, VkError>
    {
        //let entry = unsafe { Entry::load().unwrap() };
        let entry = Entry::linked(); // Dev only
        let instance = create_instance(&entry)?;
        let (debug_utils_loader, debug_callback) = create_debug_messenger(&entry, &instance)?;
        let (surface_loader, vk_surface) = presentation::create_surface(&entry, &instance, hwnd, h_instance)?;
        // Just get the first device
        let (physical_device, surface_settings) = match get_physical_devices(&instance, &surface_loader, vk_surface)?.get(0)
        {
            Some((physical_device, surface_settings)) => {
                println!("Selected device {}", physical_device.device_name);
                (physical_device.to_owned(), surface_settings.to_owned())
            }
            None => return Err("No supported devices".into()),
        };
        let device = create_logical_device(&instance, &physical_device)?;

        let mut surface = presentation::Surface {
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

        let swapchain = presentation::create_swapchain(&instance, &device, &physical_device, &surface)?;

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
        })
    }
}

fn create_instance(entry: &Entry) -> Result<Instance, VkError>
{
    let app_name = CString::new(project::APP_NAME).unwrap();
    let engine_name = CString::new("No Engine").unwrap();

    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name.as_c_str())
        .application_version(vk::make_api_version(0, project::VERSION_MAJOR, project::VERSION_MINOR, 0))
        .engine_name(engine_name.as_c_str())
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_0);

    let instance_layer_properties = unsafe { entry.enumerate_instance_layer_properties() }?;

    VALIDATION_LAYERS.are_in(instance_layer_properties)?;

    let extension_properties = unsafe { entry.enumerate_instance_extension_properties(None) }?;

    EXTENSIONS.are_in(extension_properties)?;

    let extension_ptrs = EXTENSIONS.as_ptrs();
    let validation_ptrs = VALIDATION_LAYERS.as_ptrs();

    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_ptrs)
        .enabled_layer_names(&validation_ptrs);

    Ok(unsafe { entry.create_instance(&instance_info, None) }?)
}

fn create_debug_messenger(
    entry: &Entry, instance: &Instance,
) -> Result<(debug_utils::Instance, vk::DebugUtilsMessengerEXT), VkError>
{
    let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(vulkan_debug_callback));
    let debug_utils_loader = debug_utils::Instance::new(&entry, &instance);
    let debug_call_back = unsafe { debug_utils_loader.create_debug_utils_messenger(&debug_info, None) }?;
    Ok((debug_utils_loader, debug_call_back))
}

#[derive(Clone)]
pub struct SupportedPhysicalDevice
{
    pub vk_physical_device:    vk::PhysicalDevice,
    pub device_name:           String,
    pub graphics_family_index: u32,
    pub present_family_index:  u32,
}

fn get_physical_devices(
    instance: &Instance, surface_loader: &khr::surface::Instance, surface: vk::SurfaceKHR,
) -> Result<Vec<(SupportedPhysicalDevice, SurfaceSettings)>, VkError>
{
    let physical_devices = unsafe { instance.enumerate_physical_devices() }?;
    let mut supported_devices: Vec<(SupportedPhysicalDevice, SurfaceSettings)> = Vec::new();
    for physical_device in physical_devices {
        let device_properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_name = unsafe { CStr::from_ptr(device_properties.device_name.as_ptr()) }
            .to_str()
            .unwrap_or_else(|utf_error| {
                eprintln!("Error reading device name from ptr, {}", utf_error);
                "Unknown Device"
            });
        println!("Found device {}", device_name);

        let extension_properties = match unsafe { instance.enumerate_device_extension_properties(physical_device) } {
            Ok(value) => value,
            Err(vk_error) => {
                eprintln!(
                    "Error getting device {} extension properties, {}, skipping",
                    device_name,
                    vk_error.to_string()
                );
                continue;
            }
        };

        let extension_properties_names: Vec<&CStr> = extension_properties
            .iter()
            .map(|ext| unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) })
            .collect();
        let missing_extensions: Vec<String> = DEVICE_EXTENSIONS
            .into_iter()
            .filter_map(|device_extension| {
                if !extension_properties_names.contains(&device_extension) {
                    return Some(
                        device_extension
                            .to_str()
                            .unwrap_or_else(|utf_error| {
                                eprintln!("Error reading device extension &CStr as &str, {}", utf_error);
                                "Unknown Device Extension"
                            })
                            .to_string(),
                    );
                }
                None
            })
            .collect();

        if !missing_extensions.is_empty() {
            for missing_extension in missing_extensions {
                eprintln!(
                    "Device {} does not support required extension {}",
                    device_name, missing_extension
                )
            }
            eprintln!("Device {} does not support all required extensions, skipping", device_name);
            continue;
        }

        let mut graphics_family_index: u32 = 0;
        let mut present_family_index: u32 = 0;
        let mut graphics_support: bool = false;
        let mut present_support: bool = false;
        // Any queue that supports graphics and presenting will do, TODO: at least for now
        for (queue_family_properties, index) in
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) }
                .iter()
                .zip(0u32..)
        {
            if (queue_family_properties.queue_flags & vk::QueueFlags::GRAPHICS).contains(vk::QueueFlags::GRAPHICS) {
                graphics_family_index = index;
                graphics_support = true;
            }
            if unsafe { surface_loader.get_physical_device_surface_support(physical_device, index, surface) }? {
                present_family_index = index;
                present_support = true;
            }
            if graphics_support && present_support {
                break;
            }
        }

        if !graphics_support {
            eprintln!("Device {} does not support graphics queue, skipping", device_name);
            continue;
        }

        if !present_support {
            eprintln!("Device {} does not support present queue, skipping", device_name);
            continue;
        }

        let surface_settings = match presentation::get_surface_settings(physical_device, surface_loader, surface) {
            Ok(surface_settings) => surface_settings,
            Err(e) => {
                eprintln!(
                    "Device {} failed to get correct surface capabilities, {}, skipping",
                    device_name,
                    e.to_string()
                );
                continue;
            }
        };

        supported_devices.push((
            SupportedPhysicalDevice {
                vk_physical_device: physical_device,
                device_name: device_name.to_string(),
                graphics_family_index,
                present_family_index,
            },
            surface_settings,
        ));
    }

    Ok(supported_devices)
}

// Returns a DeviceQueueCreateInfo for each unique index in queue_indices
fn get_queue_create_infos<'a>(queue_indices: Vec<u32>) -> Vec<vk::DeviceQueueCreateInfo<'a>>
{
    let queue_priority: &'a [f32; 1] = &[1.0];
    queue_indices
        .iter()
        .collect::<HashSet<&u32>>()
        .iter()
        .map(|&&index| {
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(index)
                .queue_priorities(queue_priority)
        })
        .collect()
}

fn create_logical_device(instance: &Instance, physical_device: &SupportedPhysicalDevice) -> Result<ash::Device, VkError>
{
    let queue_create_infos = get_queue_create_infos(vec![
        physical_device.graphics_family_index,
        physical_device.present_family_index,
    ]);
    let device_features = vk::PhysicalDeviceFeatures::default();

    let device_extension_ptrs = DEVICE_EXTENSIONS.as_ptrs();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(queue_create_infos.as_slice())
        .enabled_features(&device_features)
        .enabled_extension_names(&device_extension_ptrs);

    Ok(unsafe { instance.create_device(physical_device.vk_physical_device, &device_info, None) }?)
}
