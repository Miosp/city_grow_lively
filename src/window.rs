use std::u32;

use anyhow::{Context, Result};
use derive_builder::Builder;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::ValidateRect,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, w},
};

const WINDOW_CLASS_NAME: PCWSTR = w!("CityGrowWindow");
const DEFAULT_TIMER_ID: usize = 1;
const DEFAULT_WINDOW_WIDTH: u32 = 1280;
const DEFAULT_WINDOW_HEIGHT: u32 = 720;

/// Extract low-order word from LPARAM
#[inline]
const fn loword(lparam: LPARAM) -> u16 {
    (lparam.0 & 0xFFFF) as u16
}

/// Extract high-order word from LPARAM
#[inline]
const fn hiword(lparam: LPARAM) -> u16 {
    ((lparam.0 >> 16) & 0xFFFF) as u16
}

/// Configuration for window creation
#[derive(Builder)]
pub struct WindowConfig {
    pub title: String,
    #[builder(default = false)]
    pub fullscreen: bool,
    #[builder(default = None)]
    pub width: Option<u32>,
    #[builder(default = None)]
    pub height: Option<u32>,
    #[builder(default = 60)]
    pub target_framerate: u32,
}

/// Trait for handling window events
pub trait WindowHandler {
    /// Called when window needs to be painted
    fn on_paint(&mut self, hwnd: HWND);

    /// Called on timer tick
    fn on_timer(&mut self, hwnd: HWND);

    /// Called when window is resized
    fn on_resize(&mut self, hwnd: HWND, width: u32, height: u32);

    /// Called when window is being destroyed
    fn on_destroy(&mut self);
}

/// Handle WM_PAINT message
fn handle_paint<H: WindowHandler>(handler: &mut H, hwnd: HWND) -> LRESULT {
    unsafe {
        let _ = ValidateRect(Some(hwnd), None);
    }
    handler.on_paint(hwnd);
    LRESULT(0)
}

/// Handle WM_TIMER message
fn handle_timer<H: WindowHandler>(handler: &mut H, hwnd: HWND) -> LRESULT {
    handler.on_timer(hwnd);
    LRESULT(0)
}

/// Handle WM_SIZE message
fn handle_size<H: WindowHandler>(handler: &mut H, hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let width = loword(lparam) as u32;
    let height = hiword(lparam) as u32;
    handler.on_resize(hwnd, width, height);
    LRESULT(0)
}

/// Handle WM_DESTROY message
fn handle_destroy<H: WindowHandler>(handler: &mut H, handler_ptr: *mut H) -> LRESULT {
    handler.on_destroy();
    unsafe {
        PostQuitMessage(0);
        let _ = Box::from_raw(handler_ptr);
    }
    LRESULT(0)
}
/// Window wrapper that manages Win32 window lifecycle
pub struct Window {
    hwnd: HWND,
}

impl Window {
    /// Create a new window with the given configuration and handler
    pub fn create<H: WindowHandler + 'static>(config: WindowConfig, handler: H) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None).context("Failed to get module handle")?;

            // Register window class
            let wc = WNDCLASSW {
                hInstance: instance.into(),
                lpszClassName: WINDOW_CLASS_NAME,
                lpfnWndProc: Some(Self::wndproc::<H>),
                style: Default::default(),
                hCursor: LoadCursorW(None, IDC_ARROW).context("Failed to load cursor")?,
                ..Default::default()
            };

            RegisterClassW(&wc); // Ignore error if already registered

            // Determine window style and dimensions based on config
            let (style, ex_style, width, height, x, y) = if config.fullscreen {
                // For fullscreen/Lively mode, let Lively resize the window
                // Initially hidden to avoid white flash, shown after first resize
                let w = config.width.unwrap_or(DEFAULT_WINDOW_WIDTH) as i32;
                let h = config.height.unwrap_or(DEFAULT_WINDOW_HEIGHT) as i32;
                (
                    WS_POPUP,         // No WS_VISIBLE - initially hidden
                    WS_EX_TOOLWINDOW, // Don't show in taskbar
                    w,
                    h,
                    0,
                    0,
                )
            } else {
                (
                    WS_OVERLAPPEDWINDOW,
                    WINDOW_EX_STYLE::default(),
                    config.width.unwrap_or(DEFAULT_WINDOW_WIDTH) as i32,
                    config.height.unwrap_or(DEFAULT_WINDOW_HEIGHT) as i32,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                )
            };

            // Box the handler on the heap to pass through lpParam
            let handler_ptr = Box::into_raw(Box::new(handler));

            let title_wide: Vec<u16> = config
                .title
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let hwnd = CreateWindowExW(
                ex_style,
                WINDOW_CLASS_NAME,
                windows::core::PCWSTR::from_raw(title_wide.as_ptr()),
                style,
                x,
                y,
                width,
                height,
                None,
                None,
                Some(instance.into()),
                Some(handler_ptr as *const _),
            )
            .context("Failed to create window")?;

            // For non-fullscreen mode, show window immediately
            if !config.fullscreen {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }

            // Start frame timer
            SetTimer(
                Some(hwnd),
                DEFAULT_TIMER_ID,
                framerate_to_interval_ms(config.target_framerate),
                None,
            );

            // Trigger initial resize for non-fullscreen mode
            // For fullscreen/Lively mode, wait for Lively to resize the window
            if !config.fullscreen {
                let mut rect = windows::Win32::Foundation::RECT::default();
                if GetClientRect(hwnd, &mut rect).is_ok() {
                    let actual_width = (rect.right - rect.left) as u32;
                    let actual_height = (rect.bottom - rect.top) as u32;

                    // Get handler and trigger resize
                    let handler_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut H;
                    if !handler_ptr.is_null() {
                        let handler = &mut *handler_ptr;
                        handler.on_resize(hwnd, actual_width, actual_height);
                    }
                }
            }

            Ok(Self { hwnd })
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Run the message loop
    pub fn run_message_loop() -> Result<()> {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            Ok(())
        }
    }

    /// Handle WM_NCCREATE to store handler pointer
    fn handle_nccreate<H: WindowHandler>(hwnd: HWND, lparam: LPARAM) -> LRESULT {
        unsafe {
            let create_struct = lparam.0 as *const CREATESTRUCTW;
            let handler_ptr = (*create_struct).lpCreateParams as isize;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, handler_ptr);
            DefWindowProcW(hwnd, WM_NCCREATE, WPARAM(0), lparam)
        }
    }

    /// Generic window procedure that routes messages to handler
    unsafe extern "system" fn wndproc<H: WindowHandler>(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Store handler pointer on WM_NCCREATE
        if msg == WM_NCCREATE {
            return Self::handle_nccreate::<H>(hwnd, lparam);
        }

        // Retrieve handler pointer
        let handler_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut H };

        if handler_ptr.is_null() {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }

        let handler = unsafe { &mut *handler_ptr };

        match msg {
            WM_PAINT => handle_paint(handler, hwnd),
            WM_TIMER => handle_timer(handler, hwnd),
            WM_SIZE => handle_size(handler, hwnd, lparam),
            WM_DESTROY => handle_destroy(handler, handler_ptr),
            WM_CLOSE => LRESULT(0), // Let host handle lifecycle
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
}

const fn framerate_to_interval_ms(fps: u32) -> u32 {
    if fps == 0 { u32::MAX } else { 1000 / fps }
}
