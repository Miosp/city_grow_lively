use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
                    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
                    D2D1_PIXEL_FORMAT,
                },
                D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_FLAT,
                D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_INTERPOLATION_MODE_LINEAR, D2D1_PRIMITIVE_BLEND_MIN,
                D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_STROKE_STYLE_PROPERTIES1, D2D1CreateFactory,
                ID2D1Bitmap1, ID2D1CommandList, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
                ID2D1Geometry, ID2D1RectangleGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
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
    core::{Interface, w},
};
use windows_numerics::Vector2;

use crate::renderer::draw_operation::DrawOperation;

pub mod draw_operation;

/// Low-level rendering backend using Direct2D + DirectComposition
pub struct Renderer {
    // Direct3D11 (foundation for Direct2D)
    d3d_device: ID3D11Device,
    _d3d_context: ID3D11DeviceContext,

    // Direct2D
    d2d_factory: ID2D1Factory1,
    d2d_device: ID2D1Device,
    d2d_context: ID2D1DeviceContext,
    d2d_bitmap: ID2D1Bitmap1,                  // Swap chain's back buffer
    intermediate_bitmap: Option<ID2D1Bitmap1>, // Intermediate render target for incremental rendering
    cached_scene_bitmap: Option<ID2D1Bitmap1>, // Cached full scene for efficient reverse animation

    // DirectWrite
    dwrite_factory: IDWriteFactory,

    // DirectComposition (for Windows 25H2)
    swap_chain: IDXGISwapChain1,
    composition_device: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _composition_visual: IDCompositionVisual,

    // Performance optimization: brush cache (using RefCell for interior mutability)
    brush_cache: RefCell<HashMap<u32, ID2D1SolidColorBrush>>,

    // Stroke style with flat caps (no rounded endpoints)
    flat_cap_stroke_style: ID2D1StrokeStyle,

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

            // Set the swap chain bitmap as the initial render target
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

            // Step 14: Create stroke style with flat caps for pixel-perfect lines
            let stroke_props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_FLAT,
                endCap: D2D1_CAP_STYLE_FLAT,
                dashCap: D2D1_CAP_STYLE_FLAT,
                ..Default::default()
            };
            let flat_cap_stroke_style: ID2D1StrokeStyle =
                d2d_factory.CreateStrokeStyle(&stroke_props, None)?.into();

            Ok(Self {
                d3d_device,
                _d3d_context: d3d_context,
                d2d_factory,
                d2d_device,
                d2d_context,
                d2d_bitmap,
                intermediate_bitmap: None,
                cached_scene_bitmap: None,
                dwrite_factory,
                swap_chain,
                composition_device,
                _composition_target: composition_target,
                _composition_visual: composition_visual,
                brush_cache: RefCell::new(HashMap::new()),
                flat_cap_stroke_style,
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
            // Disable antialiasing for pixel-perfect rendering
            self.d2d_context
                .SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            // Reset to normal blend mode (in case it was changed for erasure)
            self.d2d_context
                .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
    }

    /// Clear the render target with a color
    pub fn clear(&self, color: D2D1_COLOR_F) {
        unsafe {
            self.d2d_context.Clear(Some(&color));
        }
    }

    pub fn is_incremental(&self) -> bool {
        self.intermediate_bitmap.is_some()
    }

    pub fn incremental(&mut self) -> Result<()> {
        self.incremental_with_copy(true)
    }

    pub fn incremental_no_copy(&mut self) -> Result<()> {
        self.incremental_with_copy(false)
    }

    fn incremental_with_copy(&mut self, copy_existing: bool) -> Result<()> {
        if self.is_incremental() {
            return Ok(());
        }

        let intermediate_bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: ManuallyDrop::new(None),
        };

        let intermediate_bitmap: ID2D1Bitmap1 = unsafe {
            self.d2d_context
                .CreateBitmap(
                    D2D_SIZE_U {
                        width: self.width,
                        height: self.height,
                    },
                    None,
                    0,
                    &intermediate_bitmap_properties,
                )
                .context("Failed to create intermediate bitmap")?
        };

        // Copy current swap chain content to intermediate bitmap if requested
        // Only copy if there's existing content to preserve (e.g., after reverse animation)
        if copy_existing {
            unsafe {
                let src_rect = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_U {
                    left: 0,
                    top: 0,
                    right: self.width,
                    bottom: self.height,
                };
                let dest_point =
                    windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2U { x: 0, y: 0 };

                intermediate_bitmap
                    .CopyFromBitmap(Some(&dest_point), &self.d2d_bitmap, Some(&src_rect))
                    .context("Failed to copy swap chain content to intermediate bitmap")?;
            }
        }

        // Switch render target to intermediate bitmap
        unsafe {
            self.d2d_context.SetTarget(&intermediate_bitmap);
        }

        self.intermediate_bitmap = Some(intermediate_bitmap);

        Ok(())
    }

    pub fn non_incremental(&mut self) {
        if !self.is_incremental() {
            return;
        }

        // Clear intermediate bitmap and switch back to swap chain bitmap
        unsafe {
            self.d2d_context.SetTarget(&self.d2d_bitmap);
        }
        self.intermediate_bitmap = None;
    }

    /// Create cached scene bitmap for efficient reverse animation
    /// This bitmap stores the fully rendered scene, avoiding redraw every frame
    pub fn ensure_cached_scene_bitmap(&mut self) -> Result<()> {
        if self.cached_scene_bitmap.is_some() {
            return Ok(());
        }

        let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: ManuallyDrop::new(None),
        };

        let cached_bitmap: ID2D1Bitmap1 = unsafe {
            self.d2d_context
                .CreateBitmap(
                    D2D_SIZE_U {
                        width: self.width,
                        height: self.height,
                    },
                    None,
                    0,
                    &bitmap_properties,
                )
                .context("Failed to create cached scene bitmap")?
        };

        self.cached_scene_bitmap = Some(cached_bitmap);
        Ok(())
    }

    /// Begin drawing to the cached scene bitmap (for regenerating the scene)
    pub fn begin_draw_to_cached_scene(&mut self) -> Result<()> {
        self.ensure_cached_scene_bitmap()?;

        unsafe {
            self.d2d_context
                .SetTarget(self.cached_scene_bitmap.as_ref().unwrap());
        }

        Ok(())
    }

    /// Finish drawing to cached scene and restore normal render target
    pub fn end_draw_to_cached_scene(&mut self) {
        // Restore the appropriate render target
        unsafe {
            if let Some(intermediate) = &self.intermediate_bitmap {
                self.d2d_context.SetTarget(intermediate);
            } else {
                self.d2d_context.SetTarget(&self.d2d_bitmap);
            }
        }
    }

    /// Draw the cached scene bitmap to the current render target (fast blit)
    pub fn draw_cached_scene(&self) -> Result<()> {
        if let Some(cached_bitmap) = &self.cached_scene_bitmap {
            let dest_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.width as f32,
                bottom: self.height as f32,
            };

            unsafe {
                self.d2d_context.DrawBitmap(
                    cached_bitmap,
                    Some(&dest_rect),
                    1.0,
                    D2D1_INTERPOLATION_MODE_LINEAR,
                    None,
                    None,
                );
            }
        }
        Ok(())
    }

    /// Clear the cached scene bitmap
    pub fn clear_cached_scene(&mut self) {
        self.cached_scene_bitmap = None;
    }

    /// End a rendering frame and present to screen
    pub fn end_draw(&self) -> Result<()> {
        // Finish drawing to intermediate bitmap
        unsafe {
            self.d2d_context
                .EndDraw(None, None)
                .context("Direct2D EndDraw failed")?;
        }

        if self.is_incremental() {
            unsafe {
                // Copy intermediate bitmap to swap chain's back buffer
                self.d2d_context.SetTarget(&self.d2d_bitmap);
                self.d2d_context.BeginDraw();
            }

            let dest_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.width as f32,
                bottom: self.height as f32,
            };

            unsafe {
                self.d2d_context.DrawBitmap(
                    self.intermediate_bitmap.as_ref().unwrap(),
                    Some(&dest_rect),
                    1.0,
                    D2D1_INTERPOLATION_MODE_LINEAR,
                    None,
                    None,
                );

                self.d2d_context
                    .EndDraw(None, None)
                    .context("Failed to copy intermediate bitmap to swap chain")?;

                // Restore intermediate bitmap as render target
                self.d2d_context
                    .SetTarget(self.intermediate_bitmap.as_ref().unwrap());
            }
        }

        unsafe {
            // Present to screen
            let _ = self.swap_chain.Present(1, DXGI_PRESENT(0));

            self.composition_device
                .Commit()
                .context("DirectComposition Commit failed")?;
        }

        Ok(())
    }

    /// Create a command list from operations (for caching/replay)
    /// This must be called OUTSIDE of a BeginDraw/EndDraw pair
    pub fn create_command_list(&self, operations: &[DrawOperation]) -> Result<ID2D1CommandList> {
        use windows::Win32::Graphics::Direct2D::ID2D1CommandList;

        unsafe {
            // Create command list
            let command_list: ID2D1CommandList = self.d2d_context.CreateCommandList()?;

            // Set command list as target
            let old_target = self.d2d_context.GetTarget()?;
            self.d2d_context.SetTarget(&command_list);

            // Record drawing operations
            self.d2d_context.BeginDraw();
            self.draw_batch(operations)?;
            self.d2d_context.EndDraw(None, None)?;

            // Close command list
            command_list.Close()?;

            // Restore original target
            self.d2d_context.SetTarget(&old_target);

            Ok(command_list)
        }
    }

    /// Draw a command list
    pub fn draw_command_list(&self, command_list: &ID2D1CommandList) -> Result<()> {
        unsafe {
            self.d2d_context.DrawImage(
                command_list,
                None,
                None,
                Default::default(),
                Default::default(),
            );
        }
        Ok(())
    }

    /// Set blend mode to MIN for pixel-perfect erasure
    /// MIN mode: O = Min(S + 1-SA, D), so drawing black (0,0,0) always results in black
    /// This handles partial pixel coverage correctly unlike COPY mode
    pub fn set_min_blend(&self) {
        unsafe {
            self.d2d_context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_MIN);
        }
    }

    /// Restore normal blend mode
    pub fn set_normal_blend(&self) {
        unsafe {
            self.d2d_context
                .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
    }

    pub fn draw_line(
        &self,
        start: Vector2,
        end: Vector2,
        color: &D2D1_COLOR_F,
        thickness: f32,
    ) -> Result<()> {
        let brush = self.get_solid_brush(color)?;
        unsafe {
            self.d2d_context
                .DrawLine(start, end, &brush, thickness, &self.flat_cap_stroke_style);
        }
        Ok(())
    }

    pub fn draw_rect(&self, rect: &D2D_RECT_F, color: &D2D1_COLOR_F, thickness: f32) -> Result<()> {
        let brush = self.get_solid_brush(color)?;
        unsafe {
            self.d2d_context
                .DrawRectangle(rect, &brush, thickness, None);
        }
        Ok(())
    }

    pub fn draw_filled_rect(&self, rect: &D2D_RECT_F, color: &D2D1_COLOR_F) -> Result<()> {
        let brush = self.get_solid_brush(color)?;
        unsafe {
            self.d2d_context.FillRectangle(rect, &brush);
        }
        Ok(())
    }

    pub fn draw_polyline(
        &self,
        points: &[Vector2],
        color: &D2D1_COLOR_F,
        thickness: f32,
    ) -> Result<()> {
        let brush = self.get_solid_brush(color)?;

        // Create path geometry for the polyline
        let path_geometry = unsafe {
            self.d2d_factory
                .CreatePathGeometry()
                .context("Failed to create path geometery")?
        };

        let geometry_sink = unsafe {
            path_geometry
                .Open()
                .context("Failed to open geometry sink")?
        };

        unsafe {
            geometry_sink.AddLines(points);
        }

        unsafe {
            geometry_sink
                .Close()
                .context("Failed to close path geometry")?;
        }

        unsafe {
            self.d2d_context.DrawGeometry(
                &path_geometry,
                &brush,
                thickness,
                &self.flat_cap_stroke_style,
            );
        }
        Ok(())
    }

    /// Draw multiple operations in a batch, optimized by grouping by color and using geometry groups
    pub fn draw_batch(&self, operations: &[DrawOperation]) -> Result<()> {
        if operations.is_empty() {
            return Ok(());
        }

        // Group operations by color and type (stroke vs fill) to minimize state changes
        use std::collections::HashMap;
        #[derive(Hash, Eq, PartialEq)]
        struct DrawKey {
            color_key: u32,
            is_fill: bool,
            thickness_bits: u32, // Store thickness as bits for hashing
        }

        let mut grouped: HashMap<DrawKey, Vec<&DrawOperation>> = HashMap::new();

        for op in operations {
            let (color_key, is_fill, thickness) = match op {
                DrawOperation::Line {
                    color, thickness, ..
                } => (Self::color_to_key(color), false, *thickness),
                DrawOperation::Rect {
                    color, thickness, ..
                } => (Self::color_to_key(color), false, *thickness),
                DrawOperation::FilledRect { color, .. } => (Self::color_to_key(color), true, 0.0),
                DrawOperation::Polyline {
                    color, thickness, ..
                } => (Self::color_to_key(color), false, *thickness),
            };

            let key = DrawKey {
                color_key,
                is_fill,
                thickness_bits: thickness.to_bits(),
            };
            grouped.entry(key).or_default().push(op);
        }

        // Process each color/type group
        for (key, ops) in grouped {
            let color = Self::key_to_color(key.color_key);
            let brush = self.get_solid_brush(&color)?;

            if key.is_fill {
                // Create geometry group for filled shapes
                let geometries = self.create_fill_geometries(ops)?;
                if !geometries.is_empty() {
                    let geometry_refs: Vec<Option<ID2D1Geometry>> =
                        geometries.iter().map(|g| Some(g.clone())).collect();
                    let geometry_group = unsafe {
                        self.d2d_factory
                            .CreateGeometryGroup(D2D1_FILL_MODE_WINDING, &geometry_refs)?
                    };
                    unsafe {
                        self.d2d_context.FillGeometry(&geometry_group, &brush, None);
                    }
                }
            } else {
                // Create geometry group for stroked shapes
                let thickness = f32::from_bits(key.thickness_bits);
                let geometries = self.create_stroke_geometries(ops)?;
                if !geometries.is_empty() {
                    let geometry_refs: Vec<Option<ID2D1Geometry>> =
                        geometries.iter().map(|g| Some(g.clone())).collect();
                    let geometry_group = unsafe {
                        self.d2d_factory
                            .CreateGeometryGroup(D2D1_FILL_MODE_WINDING, &geometry_refs)?
                    };
                    unsafe {
                        self.d2d_context.DrawGeometry(
                            &geometry_group,
                            &brush,
                            thickness,
                            &self.flat_cap_stroke_style,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Create geometries for filled shapes (rectangles)
    fn create_fill_geometries(
        &self,
        operations: Vec<&DrawOperation>,
    ) -> Result<Vec<ID2D1Geometry>> {
        let mut geometries = Vec::new();

        for op in operations {
            if let DrawOperation::FilledRect { rect, .. } = op {
                let geometry: ID2D1RectangleGeometry =
                    unsafe { self.d2d_factory.CreateRectangleGeometry(rect)? };
                geometries.push(geometry.cast::<ID2D1Geometry>()?);
            }
        }

        Ok(geometries)
    }

    /// Create geometries for stroked shapes (lines, rects, polylines)
    fn create_stroke_geometries(
        &self,
        operations: Vec<&DrawOperation>,
    ) -> Result<Vec<ID2D1Geometry>> {
        let mut geometries = Vec::new();

        for op in operations {
            match op {
                DrawOperation::Line { start, end, .. } => {
                    let path = unsafe { self.d2d_factory.CreatePathGeometry()? };
                    let sink = unsafe { path.Open()? };
                    unsafe {
                        sink.BeginFigure(*start, D2D1_FIGURE_BEGIN_HOLLOW);
                        sink.AddLine(*end);
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        sink.Close()?;
                    }
                    geometries.push(path.cast::<ID2D1Geometry>()?);
                }
                DrawOperation::Rect { rect, .. } => {
                    let geometry: ID2D1RectangleGeometry =
                        unsafe { self.d2d_factory.CreateRectangleGeometry(rect)? };
                    geometries.push(geometry.cast::<ID2D1Geometry>()?);
                }
                DrawOperation::Polyline { points, .. } => {
                    if points.len() >= 2 {
                        let path = unsafe { self.d2d_factory.CreatePathGeometry()? };
                        let sink = unsafe { path.Open()? };
                        unsafe {
                            sink.BeginFigure(points[0], D2D1_FIGURE_BEGIN_HOLLOW);
                            sink.AddLines(&points[1..]);
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            sink.Close()?;
                        }
                        geometries.push(path.cast::<ID2D1Geometry>()?);
                    }
                }
                _ => {} // Skip fill operations
            }
        }

        Ok(geometries)
    }

    /// Convert a cache key back to a color
    fn key_to_color(key: u32) -> D2D1_COLOR_F {
        let a = ((key >> 24) & 0xFF) as f32 / 255.0;
        let r = ((key >> 16) & 0xFF) as f32 / 255.0;
        let g = ((key >> 8) & 0xFF) as f32 / 255.0;
        let b = (key & 0xFF) as f32 / 255.0;
        D2D1_COLOR_F { r, g, b, a }
    }

    /// Create a solid color brush
    fn create_solid_brush(&self, color: &D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        unsafe {
            self.d2d_context
                .CreateSolidColorBrush(color, None)
                .context("Failed to create solid color brush")
        }
    }

    /// Convert a color to a cache key (ARGB as u32)
    fn color_to_key(color: &D2D1_COLOR_F) -> u32 {
        let r = (color.r * 255.0).clamp(0.0, 255.0) as u32;
        let g = (color.g * 255.0).clamp(0.0, 255.0) as u32;
        let b = (color.b * 255.0).clamp(0.0, 255.0) as u32;
        let a = (color.a * 255.0).clamp(0.0, 255.0) as u32;
        (a << 24) | (r << 16) | (g << 8) | b
    }

    /// Get or create a cached brush for the given color
    pub fn get_solid_brush(&self, color: &D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        let key = Self::color_to_key(color);

        // Check if brush exists in cache
        if let Some(brush) = self.brush_cache.borrow().get(&key) {
            return Ok(brush.clone());
        }

        // Create new brush and cache it
        let brush = self.create_solid_brush(color)?;
        self.brush_cache.borrow_mut().insert(key, brush.clone());

        Ok(brush)
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

    /// Phase 3: Create a command list from operations for efficient replay
    pub fn create_command_list_from_operations(
        &self,
        operations: &[DrawOperation],
    ) -> Result<ID2D1CommandList> {
        unsafe {
            // Create command list
            let command_list: ID2D1CommandList = self
                .d2d_context
                .CreateCommandList()
                .context("Failed to create command list")?;

            // Set command list as render target
            let previous_target = self.d2d_context.GetTarget().ok();
            self.d2d_context.SetTarget(&command_list);

            // Begin recording
            self.d2d_context.BeginDraw();

            // Draw all operations
            self.draw_batch(operations)?;

            // End recording
            self.d2d_context
                .EndDraw(None, None)
                .context("Failed to end draw on command list")?;

            // Close command list
            command_list
                .Close()
                .context("Failed to close command list")?;

            // Restore previous render target
            if let Some(target) = previous_target {
                self.d2d_context.SetTarget(&target);
            }

            Ok(command_list)
        }
    }
}
