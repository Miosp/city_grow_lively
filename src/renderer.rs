use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct2D::{
                Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
                D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
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
        },
        UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
    },
    core::Interface,
    core::w,
};

/// Low-level rendering backend using Direct2D + DirectComposition
pub struct Renderer {
    // Direct3D11 (foundation for Direct2D)
    d3d_device: ID3D11Device,
    _d3d_context: ID3D11DeviceContext,

    // Direct2D
    d2d_factory: ID2D1Factory1,
    d2d_device: ID2D1Device,
    d2d_context: ID2D1DeviceContext,
    d2d_bitmap: ID2D1Bitmap1,

    // DirectWrite
    dwrite_factory: IDWriteFactory,

    // DirectComposition (for Windows 25H2)
    swap_chain: IDXGISwapChain1,
    composition_device: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _composition_visual: IDCompositionVisual,

    // Metadata
    width: u32,
    height: u32,
}

impl Renderer {
    /// Create a new renderer for the given window
    pub fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN) as u32;
            let height = GetSystemMetrics(SM_CYSCREEN) as u32;

            // Step 1: Create D3D11 device (Direct2D requires this)
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device as *mut _),
                None,
                Some(&mut context as *mut _),
            )
            .context("Failed to create D3D11 device")?;

            let d3d_device = device.context("D3D11 device is None")?;
            let d3d_context = context.context("D3D11 context is None")?;

            // Step 2: Get DXGI device
            let dxgi_device: IDXGIDevice = d3d_device
                .cast::<IDXGIDevice>()
                .context("Failed to get IDXGIDevice from D3D11 device")?;

            // Step 3: Create Direct2D factory
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                    .context("Failed to create Direct2D factory")?;

            // Step 4: Create Direct2D device
            let d2d_device: ID2D1Device = d2d_factory
                .CreateDevice(&dxgi_device)
                .context("Failed to create Direct2D device")?;

            // Step 5: Create Direct2D device context
            let d2d_context: ID2D1DeviceContext = d2d_device
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
                .context("Failed to create Direct2D device context")?;

            // Step 6: Get DXGI adapter and factory
            let adapter = dxgi_device
                .GetAdapter()
                .context("Failed to get DXGI adapter")?;
            let factory: IDXGIFactory2 =
                adapter.GetParent().context("Failed to get DXGI factory")?;

            // Step 7: Create composition swap chain
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

            let swap_chain: IDXGISwapChain1 = factory
                .CreateSwapChainForComposition(&dxgi_device, &swap_chain_desc, None)
                .context("Failed to create composition swap chain")?;

            // Step 8: Create Direct2D bitmap from swap chain buffer
            let dxgi_surface: IDXGISurface = swap_chain
                .GetBuffer(0)
                .context("Failed to get swap chain buffer")?;

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

            let d2d_bitmap: ID2D1Bitmap1 = d2d_context
                .CreateBitmapFromDxgiSurface(&dxgi_surface, Some(&bitmap_properties))
                .context("Failed to create Direct2D bitmap from DXGI surface")?;

            d2d_context.SetTarget(&d2d_bitmap);

            // Step 9: Create DirectWrite factory
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .context("Failed to create DirectWrite factory")?;

            // Step 10: Create DirectComposition device
            let composition_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)
                .context("Failed to create DirectComposition device")?;

            // Step 11: Create composition target
            let composition_target: IDCompositionTarget = composition_device
                .CreateTargetForHwnd(hwnd, true)
                .context("Failed to create composition target")?;

            // Step 12: Create composition visual
            let composition_visual: IDCompositionVisual = composition_device
                .CreateVisual()
                .context("Failed to create composition visual")?;

            // Step 13: Wire up composition tree
            composition_visual
                .SetContent(&swap_chain)
                .context("Failed to set swap chain as visual content")?;

            composition_target
                .SetRoot(&composition_visual)
                .context("Failed to set visual as composition root")?;

            composition_device
                .Commit()
                .context("Failed to commit composition changes")?;

            Ok(Self {
                d3d_device,
                _d3d_context: d3d_context,
                d2d_factory,
                d2d_device,
                d2d_context,
                d2d_bitmap,
                dwrite_factory,
                swap_chain,
                composition_device,
                _composition_target: composition_target,
                _composition_visual: composition_visual,
                width,
                height,
            })
        }
    }

    /// Get the Direct2D device context for drawing
    pub fn context(&self) -> &ID2D1DeviceContext {
        &self.d2d_context
    }

    /// Get the DirectWrite factory for text rendering
    pub fn dwrite_factory(&self) -> &IDWriteFactory {
        &self.dwrite_factory
    }

    /// Get current render target dimensions
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Begin a rendering frame
    pub fn begin_draw(&self) {
        unsafe {
            self.d2d_context.BeginDraw();
        }
    }

    /// Clear the render target with a color
    pub fn clear(&self, color: D2D1_COLOR_F) {
        unsafe {
            self.d2d_context.Clear(Some(&color));
        }
    }

    /// End a rendering frame and present to screen
    pub fn end_draw(&self) -> Result<()> {
        unsafe {
            self.d2d_context
                .EndDraw(None, None)
                .context("Direct2D EndDraw failed")?;

            let _ = self.swap_chain.Present(1, DXGI_PRESENT(0));

            self.composition_device
                .Commit()
                .context("DirectComposition Commit failed")?;

            Ok(())
        }
    }

    /// Create a solid color brush
    pub fn create_solid_brush(&self, color: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        unsafe {
            self.d2d_context
                .CreateSolidColorBrush(&color, None)
                .context("Failed to create solid color brush")
        }
    }

    /// Create a text format for rendering text
    pub fn create_text_format(
        &self,
        font_family: &str,
        font_size: f32,
    ) -> Result<IDWriteTextFormat> {
        unsafe {
            let font_family_wide: Vec<u16> = font_family
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let text_format: IDWriteTextFormat = self
                .dwrite_factory
                .CreateTextFormat(
                    windows::core::PCWSTR::from_raw(font_family_wide.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font_size,
                    w!("en-us"),
                )
                .context("Failed to create text format")?;

            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            Ok(text_format)
        }
    }
}
