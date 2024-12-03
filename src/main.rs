#![no_main]
mod graphics;

use windows::{core::*, Win32::Foundation::*, Win32::System::Console::*, Win32::UI::WindowsAndMessaging::*};

mod project
{
    pub const APP_NAME: &str = "Vulkan Project Rust";
    pub const VERSION_MAJOR: u32 = 0;
    pub const VERSION_MINOR: u32 = 1;
    pub const WINDOW_WIDTH: i32 = 640;
    pub const WINDOW_HEIGHT: i32 = 480;
}

macro_rules! WSTR {
    ($literal_string: literal) => {
        WSTR::new($literal_string)
    };
}

#[derive(Debug)]
struct WSTR(Vec<u16>);

impl WSTR
{
    pub fn new(s: &str) -> Self
    {
        let mut string = String::from(s);
        string.push('\0');
        Self(string.encode_utf16().collect())
    }

    pub fn as_pcwstr(&self) -> PCWSTR { PCWSTR::from_raw(self.0.as_ptr()) }
}

unsafe fn message_box(title: &str, message: &str)
{
    MessageBoxW(None, WSTR::new(message).as_pcwstr(), WSTR::new(title).as_pcwstr(), MB_OK);
}

unsafe extern "system" fn window_proc(hwnd: HWND, u_msg: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT
{
    match u_msg {
        WM_DESTROY => {
            PostQuitMessage(0);
        }
        _ => (),
    }
    DefWindowProcW(hwnd, u_msg, w_param, l_param)
}

#[no_mangle]
extern "system" fn wWinMain(h_instance: HINSTANCE, _h_prev_instance: HINSTANCE, _p_cmd_line: PWSTR, n_cmd_show: i32) -> i32
{
    let window_class_name = WSTR!("Window Class");
    let wc: WNDCLASSW = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: h_instance,
        lpszClassName: window_class_name.as_pcwstr(),
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            window_class_name.as_pcwstr(),
            WSTR!("Vulkan Project Rust").as_pcwstr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            project::WINDOW_WIDTH,
            project::WINDOW_HEIGHT,
            HWND::default(),
            HMENU::default(),
            h_instance,
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                message_box("Error", e.message().as_str());
                return -1;
            }
        };

        if let Err(e) = AllocConsole().and(SetConsoleTitleW(WSTR!("Vulkan Project Console").as_pcwstr())) {
            message_box("Console Error", e.message().as_str());
            return -1;
        }

        println!("Console Initialized");

        let _ = ShowWindow(hwnd, SHOW_WINDOW_CMD(n_cmd_show));

        let _vk_app: graphics::device::VkApp = match graphics::device::VkApp::new(&hwnd, &h_instance) {
            Ok(vk_app) => vk_app,
            Err(e) => {
                message_box("Vulkan Creation Error", e.to_string().as_str());
                return -1;
            }
        };

        let mut msg: MSG = MSG::default();
        while GetMessageW(&mut msg, hwnd, 0, 0).0 > 0 {
            let _ = TranslateMessage(&mut msg);
            DispatchMessageW(&mut msg);
        }
    }
    1
}
