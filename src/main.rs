#![windows_subsystem = "windows"]

use std::fs::OpenOptions;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::time::Instant;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
                },
                D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
                D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ELLIPSE,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device,
                ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush,
            },
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
                ID3D11Device, ID3D11DeviceContext,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                DWRITE_TEXT_ALIGNMENT_CENTER, DWriteCreateFactory, IDWriteFactory,
                IDWriteTextFormat,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice,
                IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
            },
            Gdi::ValidateRect,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::*,
};

fn log_file_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("city_grow.log"))
        .with_extension("log")
}

fn log(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path())
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(file, "[{}] {}", timestamp, msg);
    }
}

struct AppState {
    d3d_device: Option<ID3D11Device>,
    d3d_context: Option<ID3D11DeviceContext>,
    d2d_factory: Option<ID2D1Factory1>,
    d2d_device: Option<ID2D1Device>,
    d2d_context: Option<ID2D1DeviceContext>,
    d2d_bitmap: Option<ID2D1Bitmap1>,
    dwrite_factory: Option<IDWriteFactory>,
    text_format: Option<IDWriteTextFormat>,
    brush: Option<ID2D1SolidColorBrush>,
    swap_chain: Option<IDXGISwapChain1>,
    composition_device: Option<IDCompositionDevice>,
    composition_target: Option<IDCompositionTarget>,
    composition_visual: Option<IDCompositionVisual>,
    render_count: u32,
    initialized: bool,
    start_time: Instant,
}

impl AppState {
    fn new() -> Self {
        log("AppState::new()");
        Self {
            d3d_device: None,
            d3d_context: None,
            d2d_factory: None,
            d2d_device: None,
            d2d_context: None,
            d2d_bitmap: None,
            dwrite_factory: None,
            text_format: None,
            brush: None,
            swap_chain: None,
            composition_device: None,
            composition_target: None,
            composition_visual: None,
            render_count: 0,
            initialized: false,
            start_time: Instant::now(),
        }
    }

    fn init(&mut self, hwnd: HWND) -> bool {
        if self.initialized {
            return true;
        }

        log("init: creating D3D11 device with Direct2D and DirectComposition support");
        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN) as u32;
            let height = GetSystemMetrics(SM_CYSCREEN) as u32;

            // Step 1: Create D3D11 device (Direct2D requires this underneath)
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL_11_0;

            let hr = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device as *mut _),
                Some(&mut feature_level as *mut _),
                Some(&mut context as *mut _),
            );

            if hr.is_err() {
                log(&format!("init: D3D11CreateDevice failed: {:?}", hr));
                return false;
            }

            self.d3d_device = device;
            self.d3d_context = context;

            log("init: D3D11 device created successfully");

            // Step 2: Get DXGI device from D3D11 device
            let Some(ref d3d_device) = self.d3d_device else {
                log("init: D3D11 device is None");
                return false;
            };

            let dxgi_device: IDXGIDevice = match d3d_device.cast() {
                Ok(dev) => dev,
                Err(e) => {
                    log(&format!("init: Failed to get IDXGIDevice: {:?}", e));
                    return false;
                }
            };

            log("init: Got IDXGIDevice");

            // Step 3: Create Direct2D factory
            let d2d_factory: ID2D1Factory1 =
                match D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) {
                    Ok(factory) => factory,
                    Err(e) => {
                        log(&format!("init: D2D1CreateFactory failed: {:?}", e));
                        return false;
                    }
                };

            self.d2d_factory = Some(d2d_factory.clone());
            log("init: Direct2D factory created");

            // Step 4: Create Direct2D device from DXGI device
            let d2d_device: ID2D1Device = match d2d_factory.CreateDevice(&dxgi_device) {
                Ok(dev) => dev,
                Err(e) => {
                    log(&format!("init: CreateDevice (D2D) failed: {:?}", e));
                    return false;
                }
            };

            self.d2d_device = Some(d2d_device.clone());
            log("init: Direct2D device created");

            // Step 5: Create Direct2D device context
            let d2d_context: ID2D1DeviceContext =
                match d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) {
                    Ok(ctx) => ctx,
                    Err(e) => {
                        log(&format!("init: CreateDeviceContext (D2D) failed: {:?}", e));
                        return false;
                    }
                };

            self.d2d_context = Some(d2d_context.clone());
            log("init: Direct2D device context created");

            // Step 6: Get adapter from DXGI device
            let adapter = match dxgi_device.GetAdapter() {
                Ok(a) => a,
                Err(e) => {
                    log(&format!("init: GetAdapter failed: {:?}", e));
                    return false;
                }
            };

            log("init: Got IDXGIAdapter");

            // Step 7: Get DXGI factory from adapter
            let factory: IDXGIFactory2 = match adapter.GetParent() {
                Ok(f) => f,
                Err(e) => {
                    log(&format!("init: GetParent (factory) failed: {:?}", e));
                    return false;
                }
            };

            log("init: Got IDXGIFactory2");

            // Step 8: Create composition swap chain (windowless)
            let swap_chain_desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: 0,
            };

            let swap_chain: IDXGISwapChain1 =
                match factory.CreateSwapChainForComposition(&dxgi_device, &swap_chain_desc, None) {
                    Ok(sc) => sc,
                    Err(e) => {
                        log(&format!(
                            "init: CreateSwapChainForComposition failed: {:?}",
                            e
                        ));
                        return false;
                    }
                };

            self.swap_chain = Some(swap_chain.clone());
            log("init: Composition swap chain created");

            // Step 9: Create Direct2D bitmap from swap chain back buffer
            let dxgi_surface: IDXGISurface = match swap_chain.GetBuffer(0) {
                Ok(surface) => surface,
                Err(e) => {
                    log(&format!("init: GetBuffer (surface) failed: {:?}", e));
                    return false;
                }
            };

            let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };

            let d2d_bitmap: ID2D1Bitmap1 = match d2d_context
                .CreateBitmapFromDxgiSurface(&dxgi_surface, Some(&bitmap_properties))
            {
                Ok(bmp) => bmp,
                Err(e) => {
                    log(&format!(
                        "init: CreateBitmapFromDxgiSurface failed: {:?}",
                        e
                    ));
                    return false;
                }
            };

            self.d2d_bitmap = Some(d2d_bitmap.clone());
            d2d_context.SetTarget(&d2d_bitmap);
            log("init: Direct2D bitmap created and set as target");

            // Step 10: Create DirectWrite factory for text rendering
            let dwrite_factory: IDWriteFactory =
                match DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) {
                    Ok(factory) => factory,
                    Err(e) => {
                        log(&format!("init: DWriteCreateFactory failed: {:?}", e));
                        return false;
                    }
                };

            self.dwrite_factory = Some(dwrite_factory.clone());
            log("init: DirectWrite factory created");

            // Step 11: Create text format for rendering
            let text_format: IDWriteTextFormat = match dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                48.0,
                w!("en-us"),
            ) {
                Ok(format) => format,
                Err(e) => {
                    log(&format!("init: CreateTextFormat failed: {:?}", e));
                    return false;
                }
            };

            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            self.text_format = Some(text_format);
            log("init: Text format created");

            // Step 12: Create a solid color brush for drawing
            let brush: ID2D1SolidColorBrush = match d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.8,
                    b: 1.0,
                    a: 1.0,
                },
                None,
            ) {
                Ok(brush) => brush,
                Err(e) => {
                    log(&format!("init: CreateSolidColorBrush failed: {:?}", e));
                    return false;
                }
            };

            self.brush = Some(brush);
            log("init: Solid color brush created");

            // Step 13: Create DirectComposition device
            let composition_device: IDCompositionDevice =
                match DCompositionCreateDevice(&dxgi_device) {
                    Ok(dev) => dev,
                    Err(e) => {
                        log(&format!("init: DCompositionCreateDevice failed: {:?}", e));
                        return false;
                    }
                };

            self.composition_device = Some(composition_device.clone());
            log("init: DirectComposition device created");

            // Step 14: Create composition target for the window
            let composition_target: IDCompositionTarget =
                match composition_device.CreateTargetForHwnd(hwnd, true) {
                    Ok(target) => target,
                    Err(e) => {
                        log(&format!("init: CreateTargetForHwnd failed: {:?}", e));
                        return false;
                    }
                };

            self.composition_target = Some(composition_target.clone());
            log("init: Composition target created");

            // Step 15: Create composition visual
            let composition_visual: IDCompositionVisual = match composition_device.CreateVisual() {
                Ok(visual) => visual,
                Err(e) => {
                    log(&format!("init: CreateVisual failed: {:?}", e));
                    return false;
                }
            };

            self.composition_visual = Some(composition_visual.clone());
            log("init: Composition visual created");

            // Step 16: Set swap chain as visual content
            if let Err(e) = composition_visual.SetContent(&swap_chain) {
                log(&format!("init: SetContent failed: {:?}", e));
                return false;
            }

            log("init: Swap chain set as visual content");

            // Step 17: Set visual as root of composition target
            if let Err(e) = composition_target.SetRoot(&composition_visual) {
                log(&format!("init: SetRoot failed: {:?}", e));
                return false;
            }

            log("init: Visual set as composition root");

            // Step 18: Commit composition changes to DWM
            if let Err(e) = composition_device.Commit() {
                log(&format!("init: Commit failed: {:?}", e));
                return false;
            }

            log("init: Composition committed to DWM");

            self.initialized = true;

            log(&format!(
                "init: Direct2D + DirectComposition initialization complete, feature level: {:?}",
                feature_level
            ));
            true
        }
    }

    fn render(&mut self) {
        if !self.initialized {
            return;
        }

        unsafe {
            if let (Some(d2d_ctx), Some(sc), Some(brush), Some(text_fmt), Some(comp_dev)) = (
                &self.d2d_context,
                &self.swap_chain,
                &self.brush,
                &self.text_format,
                &self.composition_device,
            ) {
                // Begin Direct2D drawing
                d2d_ctx.BeginDraw();

                // Clear with dark blue/purple background (matching original D3D11 version)
                d2d_ctx.Clear(Some(&D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.1,
                    b: 0.2,
                    a: 1.0,
                }));

                // Get window dimensions for animation
                let width = GetSystemMetrics(SM_CXSCREEN) as f32;
                let height = GetSystemMetrics(SM_CYSCREEN) as f32;

                // Time-based animation: bouncing circle
                let elapsed = self.start_time.elapsed();
                let time_sec = elapsed.as_secs_f32();

                // Animated circle position (bouncing around screen)
                let x = (time_sec.sin() * 0.4 + 0.5) * width;
                let y = ((time_sec * 1.3).cos() * 0.4 + 0.5) * height;

                // Animated circle radius (pulsing)
                let base_radius = width.min(height) * 0.1;
                let radius = base_radius * (1.0 + (time_sec * 2.0).sin() * 0.3);

                // Animated circle color (shifting hues)
                let r = (time_sec * 0.5).sin() * 0.5 + 0.5;
                let g = ((time_sec * 0.5) + 2.0).sin() * 0.5 + 0.5;
                let b = ((time_sec * 0.5) + 4.0).sin() * 0.5 + 0.5;

                brush.SetColor(&D2D1_COLOR_F { r, g, b, a: 0.8 });

                // Draw filled ellipse
                let mut ellipse = D2D1_ELLIPSE::default();
                ellipse.point.X = x;
                ellipse.point.Y = y;
                ellipse.radiusX = radius;
                ellipse.radiusY = radius;
                d2d_ctx.FillEllipse(&ellipse, brush);

                // Draw text
                brush.SetColor(&D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                });

                let text = "City Grow - Windows 25H2\nDirect2D + DirectComposition";
                let text_wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();

                let text_rect = D2D_RECT_F {
                    left: 0.0,
                    top: height * 0.85,
                    right: width,
                    bottom: height,
                };

                d2d_ctx.DrawText(
                    &text_wide,
                    text_fmt,
                    &text_rect,
                    brush,
                    Default::default(),
                    Default::default(),
                );

                // End Direct2D drawing
                let _ = d2d_ctx.EndDraw(None, None);

                // Present the frame
                let _ = sc.Present(1, DXGI_PRESENT(0));

                // Commit DirectComposition changes to DWM (critical for Windows 25H2)
                if let Err(e) = comp_dev.Commit() {
                    if self.render_count <= 5 {
                        log(&format!("render: Commit failed: {:?}", e));
                    }
                }

                self.render_count += 1;
                if self.render_count <= 5 || self.render_count % 60 == 0 {
                    log(&format!("render: frame {}", self.render_count));
                }
            }
        }
    }
}

thread_local! {
    static STATE: std::cell::RefCell<AppState> = std::cell::RefCell::new(AppState::new());
}

#[allow(dead_code)]
fn find_lively_parent_window() -> Option<HWND> {
    unsafe {
        // Try to find Lively's wallpaper host window
        // Lively v2.x creates windows with specific class names
        let class_names = [
            w!("LCSharpApp"), // Lively Core window
            w!("WorkerW"),    // Legacy worker window
        ];

        for class_name in &class_names {
            if let Ok(hwnd) = FindWindowW(Some(class_name), None) {
                if !hwnd.0.is_null() {
                    log(&format!(
                        "Found potential Lively parent: {:?} (class: {:?})",
                        hwnd.0, class_name
                    ));
                    return Some(hwnd);
                }
            }
        }

        log("Could not find Lively parent window");
        None
    }
}

fn parse_parent_hwnd() -> Option<HWND> {
    let args: Vec<String> = std::env::args().collect();
    log(&format!("args: {:?}", args));
    log(&format!("arg count: {}", args.len()));

    // Method 1: Look for -parenthwnd flag followed by handle
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        log(&format!("Processing arg: {}", arg));
        if arg.to_lowercase() == "-parenthwnd" || arg.to_lowercase() == "/parenthwnd" {
            if let Some(handle_str) = iter.next() {
                log(&format!("Found parent handle string: {}", handle_str));
                if let Ok(handle) = handle_str.parse::<isize>() {
                    log(&format!("Parsed parent HWND: {}", handle));
                    return Some(HWND(handle as *mut _));
                } else {
                    log(&format!("Failed to parse handle: {}", handle_str));
                }
            }
        }
    }

    // Method 2: Check if any argument is just a number (Lively might pass handle directly)
    for (i, arg) in args.iter().enumerate().skip(1) {
        if let Ok(handle) = arg.parse::<isize>() {
            log(&format!("Found numeric arg at position {}: {}", i, handle));
            return Some(HWND(handle as *mut _));
        }
    }

    log("No parent HWND found");
    None
}

fn main() -> Result<()> {
    log("=== Starting ===");

    let parent_hwnd = parse_parent_hwnd();

    // Check if we're being launched by Lively even without explicit parent
    let exe_path = std::env::current_exe().ok();
    let is_lively_context = exe_path
        .as_ref()
        .and_then(|p| p.to_str())
        .map(|s| s.contains("Lively Wallpaper"))
        .unwrap_or(false);

    if is_lively_context && parent_hwnd.is_none() {
        log("WARNING: Detected Lively Wallpaper context but no parent HWND provided");
        log("This may indicate a configuration issue with LivelyInfo.json");
    }

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let window_class = w!("CityGrowWindow");

        let wc = WNDCLASSW {
            hInstance: instance.into(),
            lpszClassName: window_class,
            lpfnWndProc: Some(wndproc),
            style: CS_HREDRAW | CS_VREDRAW,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };

        RegisterClassW(&wc);

        let (style, ex_style, parent) = if let Some(p) = parent_hwnd {
            log("Running in Lively Wallpaper mode with explicit parent");
            // Lively Wallpaper mode: create child window without WS_EX_APPWINDOW to avoid taskbar
            (
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                WS_EX_NOACTIVATE,
                Some(p),
            )
        } else if is_lively_context {
            log("Running in Lively context without explicit parent - using DirectComposition");
            // Lively context but no parent: create a window compatible with Windows 25H2
            // WS_EX_NOREDIRECTIONBITMAP: Required for DirectComposition (no GDI redirection surface)
            // WS_EX_TOOLWINDOW: Avoid taskbar
            // WS_EX_LAYERED: Participate in layered window composition
            (
                WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP,
                None,
            )
        } else {
            log("Running in standalone test mode");
            // Standalone mode for testing
            (WS_POPUP | WS_VISIBLE, WINDOW_EX_STYLE::default(), None)
        };

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);

        let hwnd = CreateWindowExW(
            ex_style,
            window_class,
            w!("City Grow"),
            style,
            0,
            0,
            screen_width,
            screen_height,
            parent,
            None,
            Some(instance.into()),
            None,
        )?;

        log(&format!("window created: {:?}", hwnd.0));

        if let Some(p) = parent_hwnd {
            // Lively Wallpaper provided parent - resize to fit
            let mut rect = RECT::default();
            let _ = GetClientRect(p, &mut rect);
            log(&format!(
                "Parent client rect: {}x{}",
                rect.right - rect.left,
                rect.bottom - rect.top
            ));
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        } else if is_lively_context {
            // Set layered window attributes for opacity
            if SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).is_ok() {
                log("Set layered window to fully opaque for Lively");
            } else {
                log("WARNING: Failed to set layered window attributes");
            }
        }

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetTimer(Some(hwnd), 1, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        log("exiting");
        Ok(())
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            STATE.with(|state| {
                let mut s = state.borrow_mut();
                if s.init(hwnd) {
                    s.render();
                }
            });
            LRESULT(0)
        }
        WM_TIMER => {
            STATE.with(|state| {
                let mut s = state.borrow_mut();
                if s.init(hwnd) {
                    s.render();
                }
            });
            LRESULT(0)
        }
        WM_SIZE => {
            STATE.with(|state| {
                let mut s = state.borrow_mut();
                // Reset to reinitialize with new size (including Direct2D and DirectComposition objects)
                s.initialized = false;
                s.brush = None;
                s.text_format = None;
                s.dwrite_factory = None;
                s.d2d_bitmap = None;
                s.d2d_context = None;
                s.d2d_device = None;
                s.d2d_factory = None;
                s.composition_visual = None;
                s.composition_target = None;
                s.composition_device = None;
                s.swap_chain = None;
                s.d3d_context = None;
                s.d3d_device = None;
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_CLOSE => LRESULT(0), // Let host handle lifecycle
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
