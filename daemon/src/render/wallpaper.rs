use std::{cell::RefCell, ffi::CStr, rc::Rc};

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use image::DynamicImage;

use crate::{display_info::DisplayInfo, gl_check, render::gl};

use super::{coordinates::Coordinates, load_texture};

pub struct Wallpaper {
    pub texture: gl::types::GLuint,
    pub image_width: u32,
    pub image_height: u32,
    display_info: Rc<RefCell<DisplayInfo>>, // transparent_texture: gl::types::GLuint,
}

impl Wallpaper {
    pub const fn new(display_info: Rc<RefCell<DisplayInfo>>) -> Self {
        Self {
            texture: 0,
            image_width: 10,
            image_height: 10,
            display_info,
        }
    }

    pub fn bind(&self, gl: &gl::Gl) -> Result<()> {
        unsafe {
            gl.BindTexture(gl::TEXTURE_2D, self.texture);
            gl_check!(gl, "binding textures");
        }

        Ok(())
    }

    pub fn load_image(&mut self, gl: &gl::Gl, image: DynamicImage) -> Result<()> {
        self.image_width = image.width();
        self.image_height = image.height();

        let texture = load_texture(gl, image)?;

        unsafe {
            // Delete from memory the previous texture
            gl.DeleteTextures(1, &self.texture);
        }
        self.texture = texture;

        Ok(())
    }

    pub fn generate_vertices_coordinates_for_fit_mode(&self) -> Coordinates {
        let display_width = self.display_info.borrow().scaled_width();
        let display_height = self.display_info.borrow().scaled_height();
        let display_ratio = display_width as f32 / display_height as f32;
        let image_ratio = self.image_width as f32 / self.image_height as f32;
        if display_ratio == image_ratio {
            Coordinates::default_vec_coordinates()
        } else if display_ratio > image_ratio {
            let x = image_ratio / display_ratio;
            Coordinates::new(-x, x, Coordinates::VEC_Y_BOTTOM, Coordinates::VEC_Y_TOP)
        } else {
            let y = 1.0 - display_ratio / image_ratio;
            Coordinates::new(
                Coordinates::VEC_X_LEFT,
                Coordinates::VEC_X_RIGHT,
                Coordinates::VEC_Y_BOTTOM - y,
                Coordinates::VEC_Y_TOP + y,
            )
        }
    }
}
