use std::{ffi::CStr, rc::Rc};

use color_eyre::Result;
use image::DynamicImage;
use log::warn;

use crate::{gl_check, render::gl};

use super::load_texture;

pub struct Wallpaper {
    gl: Rc<gl::Gl>,
    texture: gl::types::GLuint,
    image_width: u32,
    image_height: u32,
}

impl Wallpaper {
    pub fn new(gl: Rc<gl::Gl>, image: DynamicImage, current: bool) -> Result<Self> {
        let image_width = image.width();
        let image_height = image.height();
        let mut texture = 0;
        unsafe {
            gl.GenTextures(1, &mut texture);
            gl.ActiveTexture(if current { gl::TEXTURE1 } else { gl::TEXTURE0 });
            gl_check!(
                gl,
                format!(
                    "Failed to activate the texture TEXTURE{}",
                    if current { 1 } else { 0 }
                )
            );
            gl.BindTexture(gl::TEXTURE_2D, texture);
            gl_check!(
                gl,
                format!(
                    "Failed to bind the texture TEXTURE{}",
                    if current { 1 } else { 0 }
                )
            );
        }
        load_texture(&gl, image)?;

        Ok(Self {
            gl,
            texture,
            image_width,
            image_height,
        })
    }

    pub fn bind(&self) -> Result<()> {
        unsafe {
            self.gl.BindTexture(gl::TEXTURE_2D, self.texture);
            gl_check!(self.gl, "Failed to bind the texture");
        }

        Ok(())
    }

    pub fn get_image_height(&self) -> u32 {
        self.image_height
    }

    pub fn get_image_width(&self) -> u32 {
        self.image_width
    }

    pub fn load_image(&mut self, image: DynamicImage, current: bool) -> Result<()> {
        self.image_width = image.width();
        self.image_height = image.height();

        unsafe {
            self.gl
                .ActiveTexture(if current { gl::TEXTURE1 } else { gl::TEXTURE0 });
            gl_check!(
                self.gl,
                format!(
                    "Failed to activate the texture TEXTURE{}",
                    if current { 1 } else { 0 }
                )
            );
            self.bind()?;
        }
        load_texture(&self.gl, image)
    }
}

impl Drop for Wallpaper {
    fn drop(&mut self) {
        unsafe { self.gl.DeleteTextures(1, &self.texture) };
        let check_err = || -> Result<()> {
            unsafe {
                gl_check!(self.gl, "Failed to delete the previous texture");
            }
            Ok(())
        };
        if let Err(err) = check_err() {
            warn!("{:?}", err.wrap_err("Failed to drop the wallpaper"));
        }
    }
}
