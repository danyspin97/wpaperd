use std::{cell::RefCell, ffi::CStr, rc::Rc};

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use image::DynamicImage;

use crate::{gl_check, render::gl, surface::DisplayInfo, wallpaper_info::BackgroundMode};

use super::{coordinates::Coordinates, load_texture};

pub struct Wallpaper {
    pub texture: gl::types::GLuint,
    image_width: u32,
    image_height: u32,
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

    pub fn generate_texture_coordinates(&self, mode: BackgroundMode) -> Coordinates {
        // adjusted_width and adjusted_height returns the rotated sizes in case
        // the display is rotated. However, openGL is drawing in the same orientation
        // as our display (i.e. we don't apply any transform here)
        // We still need the scale
        let display_width = self.display_info.borrow().scaled_width();
        let display_height = self.display_info.borrow().scaled_height();
        let display_ratio = display_width as f32 / display_height as f32;
        let image_ratio = self.image_width as f32 / self.image_height as f32;

        match mode {
            BackgroundMode::Stretch => Coordinates::default_texture_coordinates(),
            BackgroundMode::Fit => Coordinates::default_texture_coordinates(),
            BackgroundMode::Fill if display_ratio == image_ratio => {
                Coordinates::default_texture_coordinates()
            }
            BackgroundMode::Fill if display_ratio > image_ratio => {
                // Same as width calculation below , but with inverted parameters
                // This is the expanded expression
                // adjusted_height = image_width as f32 / display_ratio;
                // y = (1.0 - image_height as f32 / adjusted_height) / 2.0;
                // We can simplify by just doing display_ration / image_ratio
                let y = (1.0 - image_ratio / display_ratio) / 2.0;
                Coordinates::new(
                    Coordinates::TEX_X_LEFT,
                    Coordinates::TEX_X_RIGHT,
                    Coordinates::TEX_Y_BOTTOM + y,
                    Coordinates::TEX_Y_TOP - y,
                )
            }
            BackgroundMode::Fill => {
                // Calculte the adjusted width, i.e. the width that the image should have to
                // have the same ratio as the display
                // adjusted_width = image_height as f32 * display_ratio;
                // Calculate the ratio between the adjusted_width and the image_width
                // x = (1.0 - adjusted_width / image_width as f32) / 2.0;
                // Simplify the expression and do the same as above
                let x = (1.0 - display_ratio / image_ratio) / 2.0;
                Coordinates::new(
                    Coordinates::TEX_X_LEFT + x,
                    Coordinates::TEX_X_RIGHT - x,
                    Coordinates::TEX_Y_BOTTOM,
                    Coordinates::TEX_Y_TOP,
                )
            }
            BackgroundMode::Tile => {
                // Tile using the original image size
                let x = display_width as f32 / self.image_width as f32;
                let y = display_height as f32 / self.image_height as f32;
                Coordinates::new(Coordinates::TEX_X_LEFT, x, Coordinates::TEX_Y_BOTTOM, y)
            }
        }
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
