use std::{ffi::CStr, ops::Deref, rc::Rc};

use color_eyre::{
    eyre::{ensure, OptionExt, WrapErr},
    Result,
};
use egl::API as egl;
use image::{DynamicImage, RgbaImage};
use log::error;
use smithay_client_toolkit::reexports::client::protocol::wl_output::Transform;

use crate::{
    display_info::DisplayInfo,
    gl_check,
    render::{
        initialize_objects, load_texture,
        shader::{create_shader, FRAGMENT_SHADER_SOURCE, VERTEX_SHADER_SOURCE},
    },
    wallpaper_info::BackgroundMode,
};

use super::{gl, wallpaper::Wallpaper, Transition};

fn transparent_image() -> RgbaImage {
    RgbaImage::from_raw(1, 1, vec![0, 0, 0, 0]).unwrap()
}

fn black_image() -> RgbaImage {
    RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255]).unwrap()
}

#[derive(Debug)]
pub enum TransitionStatus {
    Started,
    Running { started: u32, progress: f32 },
    Ended,
}

pub struct Renderer {
    gl: Rc<gl::Gl>,
    pub program: gl::types::GLuint,
    vbo: gl::types::GLuint,
    eab: gl::types::GLuint,
    // milliseconds time for the transition
    pub transition_time: u32,
    prev_wallpaper: Option<Wallpaper>,
    current_wallpaper: Wallpaper,
    transparent_texture: gl::types::GLuint,
    /// contains the progress of the current animation
    transition_status: TransitionStatus,
}

impl Renderer {
    pub unsafe fn new(
        transition_time: u32,
        transition: Transition,
        display_info: &DisplayInfo,
    ) -> Result<Self> {
        let gl = Rc::new(gl::Gl::load_with(|name| {
            egl.get_proc_address(name)
                .ok_or_eyre("Cannot find openGL ES")
                .unwrap() as *const std::ffi::c_void
        }));

        let program =
            create_program(&gl, transition).wrap_err("Failed to create openGL program")?;

        let (vbo, eab) = initialize_objects(&gl).wrap_err("Failed to initialize openGL objects")?;

        let current_wallpaper = Wallpaper::new(gl.clone());

        let transparent_texture = load_texture(&gl, transparent_image().into())
            .wrap_err("Failed to load transparent image into a texture")?;

        let mut renderer = Self {
            gl,
            program,
            vbo,
            eab,
            transition_time,
            prev_wallpaper: None,
            current_wallpaper,
            transparent_texture,
            transition_status: TransitionStatus::Ended,
        };

        renderer
            .load_wallpaper(
                black_image().into(),
                BackgroundMode::Stretch,
                None,
                display_info,
            )
            .wrap_err("Failed to query image loader")?;
        renderer
            .set_projection_matrix(display_info.transform)
            .wrap_err("Failed to set projection matrix for openGL context")?;

        Ok(renderer)
    }

    #[inline]
    pub fn check_error(&self, msg: &str) -> Result<()> {
        unsafe {
            gl_check!(self.gl, msg);
        }
        Ok(())
    }

    pub unsafe fn draw(&mut self) -> Result<()> {
        self.gl.Clear(gl::COLOR_BUFFER_BIT);
        self.check_error("Failed to clear the screen")?;

        let loc = self
            .gl
            .GetUniformLocation(self.program, b"progress\0".as_ptr() as *const _);
        self.check_error("Failed to get the uniform location for progress")?;
        self.gl.Uniform1f(
            loc,
            match self.transition_status {
                TransitionStatus::Started => 0.0,
                TransitionStatus::Running {
                    started: _,
                    progress,
                } => progress,
                TransitionStatus::Ended => 1.0,
            },
        );
        self.check_error("Failed to set the progress in the openGL shader")?;

        self.gl
            .DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, std::ptr::null());
        self.check_error("Failed to draw the vertices")?;

        Ok(())
    }

    /// Update the transition status with the current time
    #[inline]
    pub fn update_transition_status(&mut self, time: u32) -> bool {
        let started = match self.transition_status {
            TransitionStatus::Started => time,
            TransitionStatus::Running {
                started,
                progress: _,
            } => started,
            TransitionStatus::Ended => return false,
        };
        let progress =
            ((time.saturating_sub(started)) as f32 / self.transition_time as f32).min(1.0);
        // Recalculate the current progress, the transition might end now
        if progress == 1.0 {
            self.transition_finished();
            false
        } else {
            self.transition_status = TransitionStatus::Running { started, progress };
            true
        }
    }

    pub fn load_wallpaper(
        &mut self,
        image: DynamicImage,
        mode: BackgroundMode,
        offset: Option<f32>,
        display_info: &DisplayInfo,
    ) -> Result<()> {
        self.prev_wallpaper = Some(std::mem::replace(
            &mut self.current_wallpaper,
            Wallpaper::new(self.gl.clone()),
        ));
        self.current_wallpaper.load_image(image)?;

        self.bind_wallpapers(mode, offset, display_info)?;

        Ok(())
    }

    fn bind_wallpapers(
        &mut self,
        mode: BackgroundMode,
        offset: Option<f32>,
        display_info: &DisplayInfo,
    ) -> Result<()> {
        self.set_mode(mode, offset, display_info)?;

        unsafe {
            self.gl.ActiveTexture(gl::TEXTURE0);
            self.check_error("Failed to activate texture TEXTURE0")?;
            self.prev_wallpaper
                .as_ref()
                .expect("Previous wallpaper must always be set")
                .bind()?;

            // current_wallpaper is already binded to TEXTURE1, as load_texture loads the image
            // there
        }

        Ok(())
    }

    pub fn set_mode(
        &mut self,
        mode: BackgroundMode,
        offset: Option<f32>,
        display_info: &DisplayInfo,
    ) -> Result<()> {
        let display_width = display_info.scaled_width() as f32;
        let display_height = display_info.scaled_height() as f32;
        let display_ratio = display_width / display_height;
        let gen_texture_scale = |image_width: f32, image_height: f32| {
            let image_ratio: f32 = image_width / image_height;
            Box::new(match mode {
                BackgroundMode::Stretch => [1.0, 1.0],
                BackgroundMode::Center => [
                    (display_ratio / image_ratio).min(1.0),
                    (image_ratio / display_ratio).min(1.0),
                ],
                BackgroundMode::Fit | BackgroundMode::FitBorderColor => {
                    // Portrait mode
                    // In this case we calculate the width relative to the height of the
                    // screen with the ratio of the image
                    let width = display_height * image_ratio;
                    // Same thing as above, just with the width
                    let height = display_width / image_ratio;
                    // Then we calculate the proportions
                    [
                        (display_width / width).max(1.0),
                        (display_height / height).max(1.0),
                    ]
                }
                BackgroundMode::Tile => {
                    let width_proportion = display_width / image_width * display_ratio;
                    let height_proportion = display_height / image_height * display_ratio;
                    if display_ratio > image_ratio {
                        // Portrait mode
                        if height_proportion.max(1.0) == 1.0 {
                            // Same as Fit
                            let width = display_height * image_ratio;
                            [display_width / width, 1.0]
                        } else {
                            [width_proportion, height_proportion]
                        }
                    } else {
                        // Landscape mode
                        if width_proportion.max(1.0) == 1.0 {
                            // Same as Fit
                            let height = display_width / image_ratio;
                            [1.0, display_height / height]
                        } else {
                            [width_proportion, height_proportion]
                        }
                    }
                }
            })
        };
        let texture_scale = gen_texture_scale(
            self.current_wallpaper.get_image_width() as f32,
            self.current_wallpaper.get_image_height() as f32,
        );
        let (prev_image_width, prev_image_height) = if let Some(prev_wp) = &self.prev_wallpaper {
            (
                prev_wp.get_image_width() as f32,
                prev_wp.get_image_height() as f32,
            )
        } else {
            (1.0, 1.0)
        };

        let prev_texture_scale = gen_texture_scale(prev_image_width, prev_image_height);

        unsafe {
            let loc = self
                .gl
                .GetUniformLocation(self.program, b"textureScale\0".as_ptr() as *const _);
            self.check_error("Failed to get the uniform location for textureScale")?;
            ensure!(loc > 0, "Failed to find uniform textureScale");
            self.gl
                .Uniform2fv(loc, 1, texture_scale.as_ptr() as *const _);
            self.check_error("Failed to set uniform textureScale")?;

            let loc = self
                .gl
                .GetUniformLocation(self.program, b"prevTextureScale\0".as_ptr() as *const _);
            self.check_error("Failed to get the uniform location for prevTextureScale")?;
            ensure!(loc > 0, "Failed to find the uniform prevTextureScale");
            self.gl
                .Uniform2fv(loc, 1, prev_texture_scale.as_ptr() as *const _);
            self.check_error("Failed to set the value for prevTextureScale")?;

            let loc = self
                .gl
                .GetUniformLocation(self.program, b"ratio\0".as_ptr() as *const _);
            self.check_error("Failed to get the uniform location for ratio")?;
            self.gl.Uniform1f(loc, display_ratio);
            self.check_error("Failed to set the value for the uniform ratio")?;

            let offset = match (offset, mode) {
                (
                    None,
                    BackgroundMode::Stretch
                    | BackgroundMode::Center
                    | BackgroundMode::Fit
                    | BackgroundMode::FitBorderColor,
                ) => 0.5,
                (None, BackgroundMode::Tile) => 0.0,
                (Some(offset), _) => offset,
            };

            let loc = self
                .gl
                .GetUniformLocation(self.program, b"texture_offset\0".as_ptr() as *const _);
            self.check_error("Failed to get the location for the uniform texture_offset")?;
            self.gl.Uniform1f(loc, offset);
            self.check_error("Failed to set the value for the uniform texture_offset")?;

            let texture_wrap = match mode {
                BackgroundMode::Stretch | BackgroundMode::Center | BackgroundMode::Fit => {
                    gl::CLAMP_TO_BORDER_EXT
                }
                BackgroundMode::Tile => gl::REPEAT,
                BackgroundMode::FitBorderColor => gl::CLAMP_TO_EDGE,
            } as i32;

            self.gl.ActiveTexture(gl::TEXTURE0);
            self.check_error("Failed to activate texture TEXTURE0")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, texture_wrap);
            self.check_error("Failed to set the attribute TEXTURE_WRAP_S for TEXTURE0")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, texture_wrap);
            self.check_error("Failed to set the attribute TEXTURE_WRAP_T FOR TEXTURE0")?;

            self.gl.ActiveTexture(gl::TEXTURE1);
            self.check_error("Failed to activate texture TEXTURE1")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, texture_wrap);
            self.check_error("Failed to set the attribute TEXTURE_WRAP_S for TEXTURE1")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, texture_wrap);
            self.check_error("Failed to set the attribute TEXTURE_WRAP_T for TEXTURE1")?;
        }

        Ok(())
    }

    #[inline]
    pub fn start_transition(&mut self, transition_time: u32) {
        match self.transition_status {
            TransitionStatus::Started | TransitionStatus::Running { .. } => unreachable!(),
            TransitionStatus::Ended => self.transition_status = TransitionStatus::Started,
        }
        // Needed to skip the initial transition depending on the configuration
        self.transition_time = transition_time;
    }

    #[inline]
    pub fn clear_after_draw(&self) -> Result<()> {
        unsafe {
            // Unbind the framebuffer and renderbuffer before deleting.
            // self.gl.BindBuffer(gl::PIXEL_UNPACK_BUFFER, 0);
            // self.check_error("unbinding the unpack buffer")?;
            self.gl.BindFramebuffer(gl::FRAMEBUFFER, 0);
            self.check_error("Failed to unbind the framebuffer")?;
            self.gl.BindRenderbuffer(gl::RENDERBUFFER, 0);
            self.check_error("Failed to unbind the renderbuffer")?;
        }

        Ok(())
    }

    pub fn resize(&mut self, display_info: &DisplayInfo) -> Result<()> {
        unsafe {
            self.gl.Viewport(
                0,
                0,
                display_info.adjusted_width(),
                display_info.adjusted_height(),
            );
            self.check_error("Failed to resize the openGL viewport")
        }
    }

    #[inline]
    pub fn update_transition_time(&mut self, transition_time: u32) {
        self.transition_time = transition_time;
    }

    #[inline]
    pub fn transition_finished(&mut self) {
        self.transition_status = TransitionStatus::Ended;
        // By binding transparent pixel into the old wallpaper, we can delete the texture,
        // freeing space from the GPU
        unsafe {
            // The previous wallpaper is always binding in TEXTURE0
            self.gl.ActiveTexture(gl::TEXTURE0);
            self.gl
                .BindTexture(gl::TEXTURE_2D, self.transparent_texture);
            self.prev_wallpaper.take();
        }
    }

    #[inline]
    pub fn update_transition(&mut self, transition: Transition, transform: Transform) {
        match create_program(&self.gl, transition) {
            Ok(program) => {
                unsafe {
                    self.gl.DeleteProgram(self.program);
                }
                // Stop the transition immediately
                if self.transition_running() {
                    self.transition_finished();
                }
                self.program = program;
                unsafe {
                    if let Err(err) = self
                        .set_projection_matrix(transform)
                        .wrap_err("Failed to set the projection matrix")
                    {
                        error!("{err:?}");
                    }
                }
            }
            Err(err) => error!("{err:?}"),
        }
    }

    #[inline]
    pub fn transition_running(&self) -> bool {
        match self.transition_status {
            TransitionStatus::Started | TransitionStatus::Running { .. } => true,
            TransitionStatus::Ended => false,
        }
    }

    pub unsafe fn set_projection_matrix(&self, transform: Transform) -> Result<()> {
        let projection_matrix = projection_matrix(transform);
        let loc = self
            .gl
            .GetUniformLocation(self.program, b"projection_matrix\0".as_ptr() as *const _);
        self.check_error("Failed to get the uniform location for projection_matrix")?;
        ensure!(loc > 0, "Failed to find uniform projection_matrix");
        self.gl
            .UniformMatrix2fv(loc, 1, 0, projection_matrix.as_ptr());
        //self.gl
        //    .UniformMatrix2fv(loc, 1, 0, [1.0, 0.0, 0.0, 1.0].as_ptr());

        self.check_error("calling Uniform1i")?;

        Ok(())
    }
}

fn create_program(gl: &gl::Gl, transition: Transition) -> Result<gl::types::GLuint> {
    unsafe {
        let program = gl.CreateProgram();
        gl_check!(gl, "Failed to create openGL program");

        let vertex_shader = create_shader(gl, gl::VERTEX_SHADER, &[VERTEX_SHADER_SOURCE.as_ptr()])
            .expect("Failed to create vertices shader");
        let (uniform_callback, shader) = transition.clone().shader();
        let fragment_shader = create_shader(
            gl,
            gl::FRAGMENT_SHADER,
            &[FRAGMENT_SHADER_SOURCE.as_ptr(), shader.as_ptr()],
        )
        .wrap_err_with(|| {
            format!("Failed to create fragment shader for transisition {transition:?}")
        })?;

        gl.AttachShader(program, vertex_shader);
        gl_check!(gl, "Failed to attach vertices shader");
        gl.AttachShader(program, fragment_shader);
        gl_check!(gl, "Failed to attach fragment shader");
        gl.LinkProgram(program);
        gl_check!(gl, "Failed to link the openGL program");
        gl.DeleteShader(vertex_shader);
        gl_check!(gl, "Failed to delete the vertices shader");
        gl.DeleteShader(fragment_shader);
        gl_check!(gl, "Failed to delete the fragment shader");
        gl.UseProgram(program);
        gl_check!(gl, "Failed to switch to the newly created openGL program");

        // We need to setup the uniform each time we create a program
        let loc = gl.GetUniformLocation(program, b"u_prev_texture\0".as_ptr() as *const _);
        gl_check!(gl, "Failed to get the uniform location for u_prev_texture");
        ensure!(loc > 0, "Failed to find the uniform u_prev_texture");
        gl.Uniform1i(loc, 0);
        gl_check!(gl, "Failed to set the value for uniform u_prev_texture");
        let loc = gl.GetUniformLocation(program, b"u_texture\0".as_ptr() as *const _);
        gl_check!(gl, "Failed to get the uniform location for u_texture");
        ensure!(loc > 0, "Failed to find the uniform u_texture");
        gl.Uniform1i(loc, 1);
        gl_check!(gl, "Failed to set the value for uniform u_texture");

        uniform_callback(gl, program)?;

        Ok(program)
    }
}

#[rustfmt::skip]
fn projection_matrix(transform: Transform) -> [f32; 4] {
    match transform {
        Transform::Normal => {
            [
                1.0, 0.0,
                0.0, 1.0,
            ]
        }
        Transform::_90 => {
            [
                0.0, -1.0,
                1.0, 0.0,
            ]
        }
        Transform::_180 => {
            [
                -1.0, 0.0,
                0.0, -1.0,
            ]
        }
        Transform::_270 => {
            [
                0.0, 1.0,
                -1.0, 0.0,
            ]
        }
        Transform::Flipped => {
            [
                -1.0, 0.0,
                0.0, 1.0,
            ]
        }
        Transform::Flipped90 => {
            [
                0.0, -1.0,
                -1.0, 0.0,
            ]
        }
        Transform::Flipped180 => {
            [
                1.0, 0.0,
                0.0, -1.0,
            ]
        }
        Transform::Flipped270 => {
            [
                0.0, 1.0,
                1.0, 0.0,
            ]
        }
        _ => unreachable!()
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
            self.gl.DeleteBuffers(1, &self.eab);
            self.gl.DeleteBuffers(1, &self.vbo);
            self.gl.DeleteProgram(self.program);
        }
    }
}
