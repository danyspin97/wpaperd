use std::{cell::RefCell, ffi::CStr, ops::Deref, rc::Rc};

use color_eyre::{
    eyre::{bail, ensure, Context},
    Result,
};
use egl::API as egl;
use image::{DynamicImage, RgbaImage};
use log::error;

use crate::{
    display_info::DisplayInfo,
    gl_check,
    render::{
        initialize_objects, load_texture,
        shader::{create_shader, FRAGMENT_SHADER_SOURCE, VERTEX_SHADER_SOURCE},
    },
    wallpaper_info::BackgroundMode,
};

use super::{
    coordinates::{get_opengl_point_coordinates, Coordinates},
    gl,
    wallpaper::Wallpaper,
};

fn transparent_image() -> RgbaImage {
    RgbaImage::from_raw(1, 1, vec![0, 0, 0, 0]).unwrap()
}

pub struct Renderer {
    gl: gl::Gl,
    pub program: gl::types::GLuint,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    eab: gl::types::GLuint,
    // milliseconds time for the transition
    transition_time: u32,
    pub time_started: u32,
    display_info: Rc<RefCell<DisplayInfo>>,
    old_wallpaper: Wallpaper,
    current_wallpaper: Wallpaper,
    transparent_texture: gl::types::GLuint,
    transition_fit_changed: bool,
}

impl Renderer {
    pub const DEFAULT_TRANSITION_TIME: u32 = 300;

    pub unsafe fn new(
        image: DynamicImage,
        display_info: Rc<RefCell<DisplayInfo>>,
        transition_time: u32,
    ) -> Result<Self> {
        let gl = gl::Gl::load_with(|name| {
            egl.get_proc_address(name)
                .expect("egl.get_proc_address to work") as *const std::ffi::c_void
        });
        let vertex_shader = create_shader(&gl, gl::VERTEX_SHADER, VERTEX_SHADER_SOURCE)
            .expect("vertex shader creation succeed");
        let fragment_shader = create_shader(&gl, gl::FRAGMENT_SHADER, FRAGMENT_SHADER_SOURCE)
            .expect("fragment_shader");

        let program = gl.CreateProgram();
        gl_check!(gl, "calling CreateProgram");
        gl.AttachShader(program, vertex_shader);
        gl_check!(gl, "attach vertex shader");
        gl.AttachShader(program, fragment_shader);
        gl_check!(gl, "attach fragment shader");
        gl.LinkProgram(program);
        gl_check!(gl, "linking the program");
        gl.UseProgram(program);
        {
            // This shouldn't be needed, gl_check already checks the status of LinkProgram
            let mut status: i32 = 0;
            gl.GetProgramiv(program, gl::LINK_STATUS, &mut status as *mut _);
            ensure!(status == 1, "Program was not linked correctly");
        }
        gl_check!(gl, "calling UseProgram");
        gl.DeleteShader(vertex_shader);
        gl_check!(gl, "deleting the vertex shader");
        gl.DeleteShader(fragment_shader);
        gl_check!(gl, "deleting the fragment shader");
        gl.UseProgram(program);
        gl_check!(gl, "calling UseProgram");

        let (vao, vbo, eab) = initialize_objects(&gl)?;

        gl.Uniform1i(0, 0);
        gl_check!(gl, "calling Uniform1i");
        gl.Uniform1i(1, 1);
        gl_check!(gl, "calling Uniform1i");

        let old_wallpaper = Wallpaper::new(display_info.clone());
        let current_wallpaper = Wallpaper::new(display_info.clone());

        let transparent_texture = load_texture(&gl, transparent_image().into())?;

        let mut renderer = Self {
            gl,
            program,
            vao,
            vbo,
            eab,
            time_started: 0,
            transition_time,
            old_wallpaper,
            current_wallpaper,
            display_info,
            transparent_texture,
            transition_fit_changed: false,
        };

        renderer.load_wallpaper(image, BackgroundMode::Stretch)?;

        Ok(renderer)
    }

    #[inline]
    pub fn check_error(&self, msg: &str) -> Result<()> {
        unsafe {
            gl_check!(self.gl, msg);
        }
        Ok(())
    }

    pub unsafe fn draw(&mut self, time: u32, mode: BackgroundMode) -> Result<bool> {
        self.gl.Clear(gl::COLOR_BUFFER_BIT);
        self.check_error("clearing the screen")?;

        let mut progress =
            ((time - self.time_started) as f32 / self.transition_time as f32).min(1.0);
        let transition_going = progress != 1.0;

        match mode {
            BackgroundMode::Stretch | BackgroundMode::Center | BackgroundMode::Tile => {}
            BackgroundMode::Fit => {
                if !self.transition_fit_changed && progress > 0.5 {
                    self.gl.ActiveTexture(gl::TEXTURE0);
                    self.check_error("activating gl::TEXTURE0")?;
                    self.gl
                        .BindTexture(gl::TEXTURE_2D, self.transparent_texture);
                    self.gl.ActiveTexture(gl::TEXTURE1);
                    self.check_error("activating gl::TEXTURE0")?;
                    self.current_wallpaper.bind(&self.gl)?;

                    self.transition_fit_changed = true;
                    // This will recalculate the vertices
                    self.set_mode(mode, true)?;
                }
                if transition_going {
                    progress = (progress % 0.5) * 2.0;
                }
            }
        }

        let loc = self
            .gl
            .GetUniformLocation(self.program, b"u_progress\0".as_ptr() as *const _);
        self.check_error("getting the uniform location")?;
        self.gl.Uniform1f(loc, progress);
        self.check_error("calling Uniform1i")?;

        self.gl
            .DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, std::ptr::null());
        self.check_error("drawing the triangles")?;

        Ok(transition_going)
    }

    pub fn load_wallpaper(&mut self, image: DynamicImage, mode: BackgroundMode) -> Result<()> {
        std::mem::swap(&mut self.old_wallpaper, &mut self.current_wallpaper);
        self.current_wallpaper.load_image(&self.gl, image)?;

        match mode {
            BackgroundMode::Stretch | BackgroundMode::Center | BackgroundMode::Tile => unsafe {
                self.set_mode(mode, false)?;
                self.gl.ActiveTexture(gl::TEXTURE0);
                self.check_error("activating gl::TEXTURE0")?;
                self.old_wallpaper.bind(&self.gl)?;
                self.gl.ActiveTexture(gl::TEXTURE1);
                self.check_error("activating gl::TEXTURE1")?;
                self.current_wallpaper.bind(&self.gl)?;
            },
            BackgroundMode::Fit => unsafe {
                // We don't change the vertices, we still use the previous ones for the first half
                // of the transition
                self.gl.ActiveTexture(gl::TEXTURE0);
                self.check_error("activating gl::TEXTURE0")?;
                self.old_wallpaper.bind(&self.gl)?;
                self.gl.ActiveTexture(gl::TEXTURE1);
                self.check_error("activating gl::TEXTURE1")?;
                self.gl
                    .BindTexture(gl::TEXTURE_2D, self.transparent_texture);
            },
        }

        Ok(())
    }

    pub fn set_mode(
        &mut self,
        mode: BackgroundMode,
        half_transition_for_fit_mode: bool,
    ) -> Result<()> {
        match mode {
            BackgroundMode::Stretch | BackgroundMode::Center | BackgroundMode::Tile => {
                // The vertex data will be the default in this case
                let vec_coordinates = Coordinates::default_vec_coordinates();
                let current_tex_coord = &self.current_wallpaper.generate_texture_coordinates(mode);
                let old_tex_coord = &self.old_wallpaper.generate_texture_coordinates(mode);

                let vertex_data =
                    get_opengl_point_coordinates(vec_coordinates, current_tex_coord, old_tex_coord);

                unsafe {
                    // Update the vertex buffer
                    self.gl.BufferSubData(
                        gl::ARRAY_BUFFER,
                        0,
                        (vertex_data.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
                        vertex_data.as_ptr() as *const _,
                    );
                    self.check_error("buffering the data")?;
                }
            }
            BackgroundMode::Fit => {
                let vec_coordinates = if half_transition_for_fit_mode {
                    self.current_wallpaper
                        .generate_vertices_coordinates_for_fit_mode()
                } else {
                    self.old_wallpaper.generate_texture_coordinates(mode)
                };

                let old_tex_coord = &self.old_wallpaper.generate_texture_coordinates(mode);

                let vertex_data = get_opengl_point_coordinates(
                    vec_coordinates,
                    &Coordinates::default_texture_coordinates(),
                    old_tex_coord,
                );

                unsafe {
                    // Update the vertex buffer
                    self.gl.BufferSubData(
                        gl::ARRAY_BUFFER,
                        0,
                        (vertex_data.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
                        vertex_data.as_ptr() as *const _,
                    );
                    self.check_error("buffering the data")?;
                }
            }
        };
        Ok(())
    }

    #[inline]
    pub fn start_transition(&mut self, time: u32, new_transition_time: u32) {
        self.time_started = time;
        self.transition_time = new_transition_time;
        self.transition_fit_changed = false;
    }

    #[inline]
    pub fn clear_after_draw(&self) -> Result<()> {
        unsafe {
            // Unbind the framebuffer and renderbuffer before deleting.
            self.gl.BindBuffer(gl::PIXEL_UNPACK_BUFFER, 0);
            self.check_error("unbinding the unpack buffer")?;
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            self.check_error("unbinding the framebuffer")?;
            self.gl.BindRenderbuffer(gl::RENDERBUFFER, 0);
            self.check_error("unbinding the render buffer")?;
        }

        Ok(())
    }

    pub fn resize(&mut self) -> Result<()> {
        let info = self.display_info.borrow();
        unsafe {
            self.gl
                .Viewport(0, 0, info.adjusted_width(), info.adjusted_height());
            self.check_error("resizing the viewport")
        }
    }

    #[inline]
    pub fn update_transition_time(&mut self, transition_time: u32) {
        self.transition_time = transition_time;
    }

    #[inline]
    pub fn transition_finished(&mut self) {
        // By loading a transparent pixel into the old wallpaper, we free space from GPU memory
        if let Err(err) = self
            .old_wallpaper
            .load_image(&self.gl, transparent_image().into())
            .context("unloading the previous wallpaper")
        {
            error!("{err:?}");
        }
    }
}

impl Deref for Renderer {
    type Target = gl::Gl;

    fn deref(&self) -> &Self::Target {
        &self.gl
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.gl.DeleteTextures(1, &self.current_wallpaper.texture);
            self.gl.DeleteTextures(1, &self.old_wallpaper.texture);
            self.gl.DeleteBuffers(1, &self.eab);
            self.gl.DeleteBuffers(1, &self.vbo);
            self.gl.DeleteBuffers(1, &self.vao);
            self.gl.DeleteProgram(self.program);
        }
    }
}
