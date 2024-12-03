use crate::graphics::vk_app::GraphicsError;
use crate::graphics::vk_app::Result;
use ash::vk;
use derive_more::IntoIterator;
use std::ffi::{c_char, CStr};

#[derive(IntoIterator)]
pub struct Extensions<'a, const SIZE: usize>(pub [&'a CStr; SIZE]);

impl<'a, const SIZE: usize> Extensions<'a, SIZE>
{
    pub fn as_ptrs(&self) -> [*const c_char; SIZE] { self.0.map(|s| s.as_ptr()) }

    pub fn are_in<T: ExtensionNames>(&self, available_extensions: Vec<T>) -> Result<()>
    {
        if let Some(not_found_layers) = self
            .0
            .into_iter()
            .filter_map(|v| {
                if !available_extensions.iter().any(|a| a.get_name() == v) {
                    return Some(v.to_str().unwrap().to_string());
                }
                None
            })
            .reduce(|current_str: String, not_found_layer: String| current_str + ", " + not_found_layer.as_str())
        {
            return Err(GraphicsError::DeviceError(
                String::from("Did not find validation layer(s) ") + not_found_layers.as_str(),
            )
            .into());
        }

        Ok(())
    }
}

pub trait ExtensionNames
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
