use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub trait D2DColorExt {
    fn with_alpha(&self, alpha: f32) -> Self;

    fn black() -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

impl D2DColorExt for D2D1_COLOR_F {
    fn with_alpha(&self, alpha: f32) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a: alpha,
        }
    }
}
