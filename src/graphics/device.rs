use crate::graphics::{errors::VkAppError, presentation, vk_app::Result};
use crate::{log, project, warn};
use ash::{ext::debug_utils, khr, vk, Entry, Instance};
use std::ffi::{CStr, CString};

/// An error that describes some problem with the capabilities of a physical device or the execution of a function using a physical device
#[derive(Debug)]
pub struct DeviceError
{
    device_name: String,
    cause:       String,
}

impl std::fmt::Display for DeviceError
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        f.write_fmt(format_args!("Device Error for device {}: {}", self.device_name, self.cause))
    }
}

/// A list of Vulkan extension strings that a physical device can support
struct Extensions<'a, const SIZE: usize>(pub [&'a CStr; SIZE]);

impl<'a, const SIZE: usize> Extensions<'a, SIZE>
{
    /// Convert each &CStr to a pointer to the C string
    pub fn as_ptrs(&self) -> [*const std::ffi::c_char; SIZE] { self.0.map(|s| s.as_ptr()) }

    /// Checks if all the extension list are found within the available extensions
    ///
    /// Error returns a String of comma-separated requested extensions that were not found within the available extensions
    pub fn are_in<T: ExtensionNames>(&self, available_extensions: Vec<T>) -> std::result::Result<(), String>
    {
        if let Some(not_found_layers) = self
            .0
            .into_iter()
            .filter_map(|requested_extension| {
                if !available_extensions.iter().any(|a| a.get_name() == requested_extension) {
                    return Some(requested_extension.to_str().unwrap().to_string());
                }
                None
            })
            .reduce(|current_str: String, not_found_layer: String| current_str + ", " + not_found_layer.as_str())
        {
            return Err(not_found_layers);
        }

        Ok(())
    }
}

impl<'a, const SIZE: usize> IntoIterator for Extensions<'a, SIZE>
{
    type Item = &'a CStr;
    type IntoIter = std::array::IntoIter<&'a CStr, SIZE>;

    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}

trait ExtensionNames
{
    fn get_name(&self) -> &CStr;
}

impl ExtensionNames for vk::LayerProperties
{
    fn get_name(&self) -> &CStr { self.layer_name_as_c_str().expect("No NUL byte in enumerated layer") }
}

impl ExtensionNames for vk::ExtensionProperties
{
    fn get_name(&self) -> &CStr { self.extension_name_as_c_str().expect("No NUL byte in enumerated layer") }
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
        std::borrow::Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        std::borrow::Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    println!("{message_severity:?}:\n{message_type:?} [{message_id_name} ({message_id_number})] : {message}\n",);

    vk::FALSE
}

/// Initialize the Vulkan library by creating a connection between the application and the Vulkan library
pub fn create_instance(entry: &Entry) -> Result<Instance>
{
    let app_name = CString::new(project::APP_NAME).unwrap();
    let engine_name = CString::new("No Engine").unwrap();

    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name.as_c_str())
        .application_version(vk::make_api_version(0, project::VERSION_MAJOR, project::VERSION_MINOR, 0))
        .engine_name(engine_name.as_c_str())
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_0);

    // A validation layer is a debugging tool that hooks into Vulkan function calls to apply additional operations
    // TODO: Should only request and enable validation layers if in DEBUG mode
    let instance_layer_properties = unsafe { entry.enumerate_instance_layer_properties() }?;
    VALIDATION_LAYERS.are_in(instance_layer_properties).map_err(|err_string| {
        VkAppError::InstanceError(format!("Did not find requested validation layer(s) {}", err_string))
    })?;

    // An instance extension is a non-device related extension
    let extension_properties = unsafe { entry.enumerate_instance_extension_properties(None) }?;
    EXTENSIONS
        .are_in(extension_properties)
        .map_err(|err_string| VkAppError::InstanceError(format!("Did not find requested extension(s) {}", err_string)))?;

    let extension_ptrs = EXTENSIONS.as_ptrs();
    let validation_ptrs = VALIDATION_LAYERS.as_ptrs();

    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_ptrs)
        .enabled_layer_names(&validation_ptrs);

    Ok(unsafe { entry.create_instance(&instance_info, None) }?)
}

pub fn create_debug_messenger(
    entry: &Entry, instance: &Instance,
) -> Result<(debug_utils::Instance, vk::DebugUtilsMessengerEXT)>
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

/// Describes a device that has the necessary capabilities to be used for our Vulkan app
#[derive(Clone)]
pub struct SupportedPhysicalDevice
{
    pub vk_physical_device:    vk::PhysicalDevice,
    pub device_name:           String,
    pub graphics_family_index: u32,
    pub present_family_index:  u32,
}

/// Enumerates the available physical devices and returns a list of them and the device's corresponding swapchain settings
pub fn get_physical_devices(
    instance: &Instance, surface_loader: &khr::surface::Instance, surface: vk::SurfaceKHR,
) -> Result<Vec<(SupportedPhysicalDevice, presentation::SurfaceDetails)>>
{
    let physical_devices = unsafe { instance.enumerate_physical_devices() }?;
    let mut supported_devices: Vec<(SupportedPhysicalDevice, presentation::SurfaceDetails)> = Vec::new();
    for physical_device in physical_devices {
        let device_properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_name = unsafe { CStr::from_ptr(device_properties.device_name.as_ptr()) }
            .to_str()
            .unwrap_or_else(|utf_error| {
                warn!("Error reading device name from ptr, {}", utf_error);
                "Unknown Device"
            });
        log!("Found device {}", device_name);

        // The device must have the extensions we requested
        let extension_properties = match unsafe { instance.enumerate_device_extension_properties(physical_device) } {
            Ok(value) => value,
            Err(vk_error) => {
                warn!(
                    "Error getting device {} extension properties: {}, skipping",
                    device_name,
                    vk_error.to_string()
                );
                continue;
            }
        };
        if let Err(err_string) = DEVICE_EXTENSIONS.are_in(extension_properties) {
            warn!(
                "Device {} does not have required device extension(s): {}, skipping",
                device_name, err_string
            );
            continue;
        }

        /* Almost every operation in Vulkan requires commands to be submitted to a queue
           There are different types from queues which come from different queue families
           Each queue family allows only a subset of commands
           We check which queue families are supported by the device and which one(s) support the commands we want to use
        */
        let mut graphics_family_index: u32 = 0;
        let mut present_family_index: u32 = 0;
        let mut graphics_support: bool = false;
        let mut present_support: bool = false;

        // For now, we just try and look for any queue family that support graphics and presenting to a surface
        // TODO: Investigate if using the same queue family for graphics and presenting is more efficient
        for (queue_family_properties, index) in
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) }
                .iter()
                .zip(0u32..)
        // Zip with u32 instead of enumerate() as vk::QueueFlags is u32
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
            warn!("Device {} does not support graphics queue, skipping", device_name);
            continue;
        }

        if !present_support {
            warn!("Device {} does not support present queue, skipping", device_name);
            continue;
        }

        let surface_details = match presentation::get_surface_details(physical_device, surface, surface_loader) {
            Ok(surface_details) => surface_details,
            Err(e) => {
                warn!(
                    "Device {} failed to get acceptable surface capabilities, {}, skipping",
                    device_name,
                    e.to_string()
                );
                continue;
            }
        };

        let physical_device_features = unsafe { instance.get_physical_device_features(physical_device) };
        if physical_device_features.sampler_anisotropy == vk::FALSE {
            warn!("Device {} does not support sampler anisotropy, skipping", device_name);
        }

        supported_devices.push((
            SupportedPhysicalDevice {
                vk_physical_device: physical_device,
                device_name: device_name.to_string(),
                graphics_family_index,
                present_family_index,
            },
            surface_details,
        ));
    }

    Ok(supported_devices)
}

/// The DeviceQueueCreateInfo structure describes the number of queues we want for a single queue family
///
/// We only create one queue per family. We can create all the command buffers on multiple threads then submit then all at once on the main thread
///
/// Returns a DeviceQueueCreateInfo struct for each unique index in queue_family_indices
fn get_queue_create_infos<'a>(queue_family_indices: Vec<u32>) -> Vec<vk::DeviceQueueCreateInfo<'a>>
{
    // The priority influences the scheduling of command buffer execution but we only have one queue so set to 1 (max)
    let queue_priority: &'a [f32; 1] = &[1.0];
    queue_family_indices
        .iter()
        .collect::<std::collections::HashSet<&u32>>()
        .iter()
        .map(|&&index| {
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(index)
                .queue_priorities(queue_priority)
        })
        .collect()
}

/// A logical device interfaces with the selected physical device
pub fn create_logical_device(instance: &Instance, physical_device: &SupportedPhysicalDevice) -> Result<ash::Device>
{
    let queue_create_infos = get_queue_create_infos(vec![
        physical_device.graphics_family_index,
        physical_device.present_family_index,
    ]);

    // We require anisotropy
    // TODO: Make an option
    let device_features = vk::PhysicalDeviceFeatures::default().sampler_anisotropy(true);

    // At this point we should know that the physical device supports the requested device extensions so we don't need to check again
    let device_extension_ptrs = DEVICE_EXTENSIONS.as_ptrs();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(queue_create_infos.as_slice())
        .enabled_features(&device_features)
        .enabled_extension_names(&device_extension_ptrs);

    Ok(unsafe { instance.create_device(physical_device.vk_physical_device, &device_info, None) }?)
}
