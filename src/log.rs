use crate::message_box;

#[macro_export]
macro_rules! log {
        ($($arg:tt)*) => {
        println!("[Info] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        eprintln!("[Warning] {}", format!($($arg)*))
    };
}

pub trait ProjectError: std::error::Error
{
    fn title(&self) -> String;

    /// Function for default handling an error
    ///
    /// Prints error message to console and displays a message box with the error
    fn handle(&self)
    {
        eprintln!("{:->50}", '-');
        eprintln!("[{} Error] {}", self.title(), self.to_string());
        eprintln!("{:->50}", '-');
        unsafe {
            message_box(format!("{} Error", self.title()).as_str(), self.to_string().as_str());
        }
        // TODO: Create log file
    }
}
