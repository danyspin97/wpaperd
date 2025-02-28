use smithay_client_toolkit::{
    output::OutputInfo, reexports::client::protocol::wl_output::Transform,
    shell::wlr_layer::LayerSurfaceConfigure,
};

#[derive(Debug)]
pub struct DisplayInfo {
    pub name: String,
    pub description: String,
    pub width: i32,
    pub height: i32,
    pub scale: i32,
    pub transform: Transform,
}

impl DisplayInfo {
    pub fn new(info: OutputInfo) -> Self {
        Self {
            name: info.name.unwrap_or_default(),
            description: info.description.unwrap_or_default(),
            width: 0,
            height: 0,
            scale: info.scale_factor,
            transform: info.transform,
        }
    }

    #[inline]
    pub fn adjusted_width(&self) -> i32 {
        match self.transform {
            Transform::Normal | Transform::_180 | Transform::Flipped | Transform::Flipped180 => {
                self.width * self.scale
            }
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
                self.height * self.scale
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn adjusted_height(&self) -> i32 {
        match self.transform {
            Transform::Normal | Transform::_180 | Transform::Flipped | Transform::Flipped180 => {
                self.height * self.scale
            }
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
                self.width * self.scale
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn scaled_width(&self) -> i32 {
        self.width * self.scale
    }

    #[inline]
    pub fn scaled_height(&self) -> i32 {
        self.height * self.scale
    }

    #[inline]
    #[allow(unused)]
    pub fn ratio(&self) -> f32 {
        self.adjusted_width() as f32 / self.adjusted_height() as f32
    }

    pub fn change_size(&mut self, configure: LayerSurfaceConfigure) -> bool {
        let new_width = configure.new_size.0 as i32;
        let new_height = configure.new_size.1 as i32;
        if (self.width, self.height) != (new_width, new_height) {
            self.width = new_width;
            self.height = new_height;
            true
        } else {
            false
        }
    }

    pub fn change_transform(&mut self, transform: Transform) -> bool {
        if self.transform != transform {
            self.transform = transform;
            true
        } else {
            false
        }
    }

    pub fn change_scale_factor(&mut self, scale_factor: i32) -> bool {
        if self.scale != scale_factor {
            self.scale = scale_factor;
            true
        } else {
            false
        }
    }
}
