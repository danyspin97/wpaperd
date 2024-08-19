use std::{ffi::CStr, rc::Rc};

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use image::DynamicImage;

use crate::{gl_check, render::gl};

use super::load_texture;

pub struct Wallpaper {
    gl: Rc<gl::Gl>,
    texture: gl::types::GLuint,
    image_width: u32,
    image_height: u32,
}

impl Wallpaper {
    pub const fn new(gl: Rc<gl::Gl>) -> Self {
        Self {
            gl,
            texture: 0,
            image_width: 10,
            image_height: 10,
        }
    }

    pub fn bind(&self) -> Result<()> {
        unsafe {
            self.gl.BindTexture(gl::TEXTURE_2D, self.texture);
            gl_check!(self.gl, "binding textures");
        }

        Ok(())
    }

    pub fn load_image(&mut self, image: DynamicImage) -> Result<()> {
        self.image_width = image.width();
        self.image_height = image.height();

        let texture = load_texture(&self.gl, image)?;

        unsafe {
            // Delete from memory the previous texture
            self.gl.DeleteTextures(1, &self.texture);
        }
        self.texture = texture;

        Ok(())
    }

    pub fn get_image_height(&self) -> u32 {
        self.image_height
    }

    pub fn get_image_width(&self) -> u32 {
        self.image_width
    }
}

impl Drop for Wallpaper {
    fn drop(&mut self) {
        unsafe { self.gl.DeleteTextures(1, &self.texture) };
    }
}
