use crate::log;
use ash::vk;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum VkAppError
{
    VkError(vk::Result),
    IoError(std::io::Error, String),
    /// An
    InstanceError(String),
    /// An error rasied when
    DeviceError(String),
}

impl log::ProjectError for VkAppError
{
    fn title(&self) -> String
    {
        String::from(match *self {
            VkAppError::VkError(_) => "Vulkan",
            VkAppError::IoError(_, _) => "IO",
            VkAppError::DeviceError(_) => "Device",
            VkAppError::InstanceError(_) => "Instance",
        })
    }
}

impl Display for VkAppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match *self {
            VkAppError::VkError(ref err) => write!(f, "Code {}: {}", err.as_raw(), err.to_string()),
            VkAppError::IoError(ref err, ref file) => write!(f, "{} for file {}", err.to_string(), file),
            VkAppError::DeviceError(ref err) => write!(f, "{}", err.to_string()),
            VkAppError::InstanceError(ref err) => write!(f, "{}", err.to_string()),
        }
    }
}

impl std::error::Error for VkAppError
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)>
    {
        match *self {
            VkAppError::VkError(ref err) => Some(err),
            VkAppError::IoError(ref err, _) => Some(err),
            VkAppError::InstanceError(_) => None,
            VkAppError::DeviceError(_) => None,
        }
    }
}

impl From<vk::Result> for VkAppError
{
    fn from(vk_result: vk::Result) -> Self
    {
        VkAppError::VkError(
            vk_result
                .result()
                .expect_err("Trying to unwrap successful Vulkan Result as error"),
        )
    }
}

pub trait IOResultToResultExt<T>
{
    fn to_result(self, path: &str) -> crate::graphics::vk_app::Result<T>;
}

impl<T> IOResultToResultExt<T> for std::io::Result<T>
{
    fn to_result(self, path: &str) -> crate::graphics::vk_app::Result<T>
    {
        self.map_err(|err| VkAppError::IoError(err, path.to_string()))
    }
}
