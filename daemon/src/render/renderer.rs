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
    Transition,
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
    pub unsafe fn new(
        image: DynamicImage,
        display_info: Rc<RefCell<DisplayInfo>>,
        transition_time: u32,
        transition: Transition,
    ) -> Result<Self> {
        let gl = gl::Gl::load_with(|name| {
            egl.get_proc_address(name)
                .expect("egl.get_proc_address to work") as *const std::ffi::c_void
        });

        let program = create_program(&gl, transition)
            .context("unable to create program during openGL ES initialization")?;

        let (vao, vbo, eab) = initialize_objects(&gl)?;

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

        let mut progress = ((time.saturating_sub(self.time_started)) as f32
            / self.transition_time as f32)
            .min(1.0);
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
                    self.check_error("activating gl::TEXTURE1")?;
                    self.current_wallpaper.bind(&self.gl)?;

                    self.transition_fit_changed = true;
                    // This will recalculate the vertices
                    self.set_mode(mode, true)?;
                    return Ok(transition_going);
                }
                if transition_going {
                    progress = (progress % 0.5) * 2.0;
                }
            }
        }

        let loc = self
            .gl
            .GetUniformLocation(self.program, b"progress\0".as_ptr() as *const _);
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

        self.bind_wallpapers(mode)?;

        Ok(())
    }

    fn bind_wallpapers(&mut self, mode: BackgroundMode) -> Result<()> {
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
        };

        Ok(())
    }

    pub fn set_mode(
        &mut self,
        mode: BackgroundMode,
        current_vertices_for_fit_mode: bool,
    ) -> Result<()> {
        let (vertices, texture_scale, prev_texture_scale) = match mode {
            BackgroundMode::Stretch | BackgroundMode::Center | BackgroundMode::Tile => {
                let ratio = self.display_info.borrow().ratio();
                let current_image_ratio = self.current_wallpaper.image_height as f32
                    / self.current_wallpaper.image_width as f32;
                let aspect = current_image_ratio / ratio;
                let texture_scale = Box::new(match mode {
                    BackgroundMode::Stretch => [1.0, 1.0],
                    BackgroundMode::Center => [1.0, 1.0 / aspect],
                    BackgroundMode::Fit => unreachable!(),
                    BackgroundMode::Tile => [aspect, 1.0],
                });
                let prev_image_ratio =
                    self.old_wallpaper.image_height as f32 / self.old_wallpaper.image_width as f32;
                let aspect = prev_image_ratio / ratio;
                let prev_texture_scale = Box::new(match mode {
                    BackgroundMode::Stretch => [1.0, 1.0],
                    BackgroundMode::Center => [1.0, 1.0 / aspect],
                    BackgroundMode::Fit => unreachable!(),
                    BackgroundMode::Tile => [aspect, 1.0],
                });

                (
                    Coordinates::default_vec_coordinates(),
                    texture_scale,
                    prev_texture_scale,
                )
            }
            BackgroundMode::Fit => {
                let vec_coordinates = if current_vertices_for_fit_mode {
                    self.current_wallpaper
                        .generate_vertices_coordinates_for_fit_mode()
                } else {
                    self.old_wallpaper
                        .generate_vertices_coordinates_for_fit_mode()
                };

                let texture_scale = Box::new([1.0, 1.0]);

                (vec_coordinates, texture_scale.clone(), texture_scale)
            }
        };

        let vertex_data =
            get_opengl_point_coordinates(vertices, Coordinates::default_texture_coordinates());

        unsafe {
            // Update the vertex buffer
            self.gl.BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (vertex_data.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
                vertex_data.as_ptr() as *const _,
            );
            self.check_error("buffering the data")?;

            let loc = self
                .gl
                .GetUniformLocation(self.program, b"textureScale\0".as_ptr() as *const _);
            self.check_error("getting the uniform location")?;
            ensure!(loc > 0, "textureScale not found");
            self.gl
                .Uniform2fv(loc, 1, texture_scale.as_ptr() as *const _);
            self.check_error("calling Uniform2fv on textureScale")?;

            let loc = self
                .gl
                .GetUniformLocation(self.program, b"prevTextureScale\0".as_ptr() as *const _);
            self.check_error("getting the uniform location")?;
            ensure!(loc > 0, "prevTextureScale not found");
            self.gl
                .Uniform2fv(loc, 1, prev_texture_scale.as_ptr() as *const _);
            self.check_error("calling Uniform2fv on prevTextureScale")?;

            let display_info = self.display_info.borrow();
            let ratio =
                display_info.adjusted_width() as f32 / display_info.adjusted_height() as f32;

            let loc = self
                .gl
                .GetUniformLocation(self.program, b"ratio\0".as_ptr() as *const _);
            self.check_error("getting the uniform location")?;
            self.gl.Uniform1f(loc, ratio);
            self.check_error("calling Uniform1f")?;
        }

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

    #[inline]
    pub fn update_transition(&mut self, transition: Transition) {
        match create_program(&self.gl, transition) {
            Ok(program) => {
                unsafe {
                    self.gl.DeleteProgram(self.program);
                }
                self.program = program;
            }
            Err(err) => error!("{err:?}"),
        }
    }
}

fn create_program(gl: &gl::Gl, transition: Transition) -> Result<gl::types::GLuint> {
    unsafe {
        let program = gl.CreateProgram();
        gl_check!(gl, "calling CreateProgram");

        let vertex_shader = create_shader(gl, gl::VERTEX_SHADER, &[VERTEX_SHADER_SOURCE.as_ptr()])
            .expect("vertex shader creation succeed");
        let (uniform_callback, shader) = transition.clone().shader();
        let fragment_shader = create_shader(
            gl,
            gl::FRAGMENT_SHADER,
            &[FRAGMENT_SHADER_SOURCE.as_ptr(), shader.as_ptr()],
        )
        .with_context(|| {
            format!("unable to create fragment_shader with transisition {transition:?}")
        })?;

        gl.AttachShader(program, vertex_shader);
        gl_check!(gl, "attach vertex shader");
        gl.AttachShader(program, fragment_shader);
        gl_check!(gl, "attach fragment shader");
        gl.LinkProgram(program);
        gl_check!(gl, "linking the program");
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

        // We need to setup the uniform each time we create a program
        let loc = gl.GetUniformLocation(program, b"u_prev_texture\0".as_ptr() as *const _);
        gl_check!(gl, "getting the uniform location for u_prev_texture");
        ensure!(loc > 0, "u_prev_texture not found");
        gl.Uniform1i(loc, 0);
        gl_check!(gl, "calling Uniform1i");
        let loc = gl.GetUniformLocation(program, b"u_texture\0".as_ptr() as *const _);
        gl_check!(gl, "getting the uniform location for u_texture");
        ensure!(loc > 0, "u_texture not found");
        gl.Uniform1i(loc, 1);
        gl_check!(gl, "calling Uniform1i");

        uniform_callback(gl, program)?;

        Ok(program)
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
