use crate::graphics::{device, pipeline, presentation};
use ash::ext::debug_utils;
use ash::{vk, Entry, Instance};
use std::error::Error;
use std::{fmt, io};

pub struct Vertex
{
    pub position:  [f32; 2],
    pub colour:    [f32; 3],
    pub tex_coord: [f32; 2],
}

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
    _entry:             Entry, // For loading vulkan, must have same lifetime as struct
    instance:           Instance,
    debug_utils_loader: debug_utils::Instance,
    debug_callback:     vk::DebugUtilsMessengerEXT,
    physical_device:    device::SupportedPhysicalDevice,
    surface:            presentation::Surface,
    device:             ash::Device,
    graphics_queue:     vk::Queue,
    present_queue:      vk::Queue,
    swapchain:          presentation::Swapchain,
    pipeline:           pipeline::Pipeline,
}

impl Drop for VkApp
{
    fn drop(&mut self)
    {
        println!("Cleaning up");
        unsafe {
            self.pipeline.cleanup(&self.device);
            self.swapchain.cleanup(&self.device);
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

        let swapchain = presentation::create_swapchain(&instance, &device, &physical_device, &surface)?;

        let pipeline = pipeline::create_pipeline(&device, surface_settings, &swapchain)?;

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
        })
    }
}
