use std::borrow::Cow;
use std::ffi::{self, CString};
use std::ops::Deref;
use ash::{vk, khr, Entry, Instance, ext::debug_utils};
use windows;
use crate::graphics::extensions::*;
use crate::{message_box, project};

const VALIDATION_LAYERS: Extensions<1> = Extensions([c"VK_LAYER_KHRONOS_validation"]);
const EXTENSIONS: Extensions<3> = Extensions([vk::KHR_SURFACE_NAME, vk::EXT_DEBUG_UTILS_NAME, vk::KHR_WIN32_SURFACE_NAME]);

unsafe extern "system" fn vulkan_debug_callback(message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT, p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message_id_number = callback_data.message_id_number;

    let message_id_name = if callback_data.p_message_id_name.is_null() {
        Cow::from("")
    } else {
        ffi::CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        Cow::from("")
    } else {
        ffi::CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    println!(
        "{message_severity:?}:\n{message_type:?} [{message_id_name} ({message_id_number})] : {message}\n",
    );

    message_box("Vulkan Debug Message", message.deref());

    vk::FALSE
}

pub struct VkApp {
    _entry: Entry, // For loading vulkan, must have same lifetime as struct
    instance: Instance,
    debug_utils_loader: debug_utils::Instance,
    debug_callback: vk::DebugUtilsMessengerEXT,
    surface_loader: khr::surface::Instance,
    surface: vk::SurfaceKHR
}

impl Drop for VkApp {
    fn drop(&mut self) {
        println!("Cleaning up");
        unsafe {
            self.surface_loader.destroy_surface(self.surface, None);
            self.debug_utils_loader.destroy_debug_utils_messenger(self.debug_callback, None);
            self.instance.destroy_instance(None);
        }
    }
}

impl VkApp {
    pub fn new(hwnd: &windows::Win32::Foundation::HWND, h_instance: &windows::Win32::Foundation::HINSTANCE) -> Result<Self, String> {
        //let entry = unsafe { Entry::load().unwrap() };
        let entry = Entry::linked(); // Dev only
        let instance = create_instance(&entry)?;
        let (debug_utils_loader, debug_callback) = create_debug_messenger(&entry, &instance)?;
        let (surface_loader, surface) = create_surface(&entry, &instance, hwnd, h_instance)?;
        Ok(Self { _entry: entry, instance, debug_utils_loader, debug_callback, surface_loader, surface })
    }
}

fn create_instance(entry: &Entry) -> Result<Instance, String> {
    let app_name = CString::new(project::APP_NAME).unwrap();
    let engine_name = CString::new("No Engine").unwrap();

    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name.as_c_str())
        .application_version(vk::make_api_version(0, project::VERSION_MAJOR, project::VERSION_MINOR, 0))
        .engine_name(engine_name.as_c_str())
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_0);

    let instance_layer_properties = unsafe { entry.enumerate_instance_layer_properties() }
        .map_err(|e| e.to_string())?;

    VALIDATION_LAYERS.are_in(instance_layer_properties)?;

    let extension_properties = unsafe { entry.enumerate_instance_extension_properties(None) }
        .map_err(|e| e.to_string())?;

    EXTENSIONS.are_in(extension_properties)?;

    let extension_ptrs = EXTENSIONS.as_ptrs();
    let validation_ptrs = VALIDATION_LAYERS.as_ptrs();

    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_ptrs)
        .enabled_layer_names(&validation_ptrs);

    Ok(unsafe { entry.create_instance(&instance_info, None) }
        .map_err(|e| e.to_string())?)
}

fn create_debug_messenger(entry: &Entry, instance: &Instance) -> Result<(debug_utils::Instance, vk::DebugUtilsMessengerEXT), String> {
    let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
            | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
        ).pfn_user_callback(Some(vulkan_debug_callback));
    let debug_utils_loader = debug_utils::Instance::new(&entry, &instance);
    let debug_call_back = unsafe { debug_utils_loader.create_debug_utils_messenger(&debug_info, None) }
        .map_err(|e| e.to_string())?;
    Ok((debug_utils_loader, debug_call_back))
}

fn create_surface(entry: &Entry, instance: &Instance,
                  hwnd: &windows::Win32::Foundation::HWND, h_instance: &windows::Win32::Foundation::HINSTANCE)
                    -> Result<(khr::surface::Instance, vk::SurfaceKHR), String> {
    let surface_info: vk::Win32SurfaceCreateInfoKHR = vk::Win32SurfaceCreateInfoKHR::default()
        .hwnd(hwnd.0 as isize)
        .hinstance(h_instance.0 as isize);

    let win32_surface_instance = khr::win32_surface::Instance::new(entry, instance);

    let surface = unsafe { win32_surface_instance.create_win32_surface(&surface_info, None) }
        .map_err(|e| e.to_string())?;

    let surface_loader = khr::surface::Instance::new(entry, instance);

    Ok((surface_loader, surface))
}