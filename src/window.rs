use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::ValidateRect,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

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

/// Window wrapper that manages Win32 window lifecycle
pub struct Window {
    hwnd: HWND,
}

impl Window {
    /// Create a new window with the given handler
    pub fn create<H: WindowHandler + 'static>(title: &str, handler: H) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None).context("Failed to get module handle")?;

            // Register window class
            let wc = WNDCLASSW {
                hInstance: instance.into(),
                lpszClassName: w!("CityGrowWindow"),
                lpfnWndProc: Some(Self::wndproc::<H>),
                style: CS_HREDRAW | CS_VREDRAW,
                hCursor: LoadCursorW(None, IDC_ARROW).context("Failed to load cursor")?,
                ..Default::default()
            };

            RegisterClassW(&wc); // Ignore error if already registered

            // Determine window style based on context
            let style = WS_POPUP | WS_VISIBLE;
            let ex_style = WINDOW_EX_STYLE::default();
            let parent = None;

            let screen_width = GetSystemMetrics(SM_CXSCREEN);
            let screen_height = GetSystemMetrics(SM_CYSCREEN);

            // Box the handler on the heap to pass through lpParam
            let handler_ptr = Box::into_raw(Box::new(handler));

            let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

            let hwnd = CreateWindowExW(
                ex_style,
                w!("CityGrowWindow"),
                windows::core::PCWSTR::from_raw(title_wide.as_ptr()),
                style,
                0,
                0,
                screen_width,
                screen_height,
                parent,
                None,
                Some(instance.into()),
                Some(handler_ptr as *const _),
            )
            .context("Failed to create window")?;

            ShowWindow(hwnd, SW_SHOW);

            // Start 60fps timer
            SetTimer(Some(hwnd), 1, 16, None);

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

    /// Generic window procedure that routes messages to handler
    unsafe extern "system" fn wndproc<H: WindowHandler>(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Store handler pointer on WM_NCCREATE
        if msg == WM_NCCREATE {
            unsafe {
                let create_struct = lparam.0 as *const CREATESTRUCTW;
                let handler_ptr = (*create_struct).lpCreateParams as isize;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, handler_ptr);
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
        }

        unsafe {
            // Retrieve handler pointer
            let handler_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut H;

            if !handler_ptr.is_null() {
                let handler = &mut *handler_ptr;

                match msg {
                    WM_PAINT => {
                        let _ = ValidateRect(Some(hwnd), None);
                        handler.on_paint(hwnd);
                        LRESULT(0)
                    }
                    WM_TIMER => {
                        handler.on_timer(hwnd);
                        LRESULT(0)
                    }
                    WM_SIZE => {
                        let width = (lparam.0 & 0xFFFF) as u32;
                        let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                        handler.on_resize(hwnd, width, height);
                        LRESULT(0)
                    }
                    WM_DESTROY => {
                        handler.on_destroy();
                        PostQuitMessage(0);

                        // Clean up handler
                        let _ = Box::from_raw(handler_ptr);

                        LRESULT(0)
                    }
                    WM_CLOSE => LRESULT(0), // Let host handle lifecycle
                    _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
    }
}
