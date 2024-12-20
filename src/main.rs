#![no_main]
mod graphics;
mod log;
mod maths;

use crate::log::ProjectError;
use std::io::Read;
use windows::{core::*, Win32::Foundation::*, Win32::System::Console::*, Win32::UI::WindowsAndMessaging::*};

mod project
{
    pub const APP_NAME: &str = "Vulkan Project Rust";
    pub const VERSION_MAJOR: u32 = 0;
    pub const VERSION_MINOR: u32 = 1;
    thread_local! {
        pub static WINDOW_WIDTH: std::cell::Cell<i32> = std::cell::Cell::new(640);
        pub static WINDOW_HEIGHT: std::cell::Cell<i32> = std::cell::Cell::new(480);
    }
}

macro_rules! WSTR {
    ($literal_string: literal) => {
        WSTR::new($literal_string)
    };
}

// Get two low order bytes of a LPARAM
fn loword(l_param: &LPARAM) -> u16 { ((l_param.0 as u64) & 0xffff) as u16 }

// Get two high order bytes of a LPARAM
fn hiword(w_param: &LPARAM) -> u16 { (((w_param.0 as u64) >> 16) & 0xffff) as u16 }

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

pub unsafe fn message_box(title: &str, message: &str)
{
    MessageBoxW(None, WSTR::new(message).as_pcwstr(), WSTR::new(title).as_pcwstr(), MB_OK);
}

unsafe extern "system" fn window_proc(hwnd: HWND, u_msg: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT
{
    match u_msg {
        WM_DESTROY => {
            PostQuitMessage(0);
        }
        WM_SIZE => {
            project::WINDOW_WIDTH.set(loword(&l_param) as i32);
            project::WINDOW_HEIGHT.set(hiword(&l_param) as i32);
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
        if let Err(e) = AllocConsole().and(SetConsoleTitleW(WSTR!("Vulkan Project Console").as_pcwstr())) {
            message_box("Console Error", e.message().as_str());
            return -1;
        }

        log!("Console Initialized");

        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            window_class_name.as_pcwstr(),
            WSTR!("Vulkan Project Rust").as_pcwstr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            project::WINDOW_WIDTH.get(),
            project::WINDOW_HEIGHT.get(),
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

        let _ = ShowWindow(hwnd, SHOW_WINDOW_CMD(n_cmd_show));

        let mut vk_app: graphics::vk_app::VkApp = match graphics::vk_app::VkApp::new(&hwnd, &h_instance) {
            Ok(vk_app) => vk_app,
            Err(err) => {
                err.handle();
                return -1;
            }
        };

        let mut msg: MSG = MSG::default();
        loop {
            let _ = TranslateMessage(&mut msg);
            DispatchMessageW(&mut msg);
            if GetMessageW(&mut msg, hwnd, 0, 0).0 > 0 {
                if let Err(err) = vk_app.draw_frame() {
                    err.handle();
                    return -1;
                }
            } else {
                break;
            }
        }
    }
    println!("Press any key to exit");
    std::io::stdin().read(&mut [0]).unwrap();
    0
}
