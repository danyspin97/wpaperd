use std::ffi::CStr;

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use image::DynamicImage;

use crate::{gl_check, render::gl};

use super::load_texture;

pub struct Wallpaper {
    pub texture: gl::types::GLuint,
    pub image_width: u32,
    pub image_height: u32,
}

impl Wallpaper {
    pub const fn new() -> Self {
        Self {
            texture: 0,
            image_width: 10,
            image_height: 10,
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
}
