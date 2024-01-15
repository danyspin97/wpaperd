use std::{
    ffi::{c_void, CStr},
    ops::Deref,
};

use color_eyre::{
    eyre::{bail, ensure, Context},
    Result,
};
use egl::API as egl;
use image::DynamicImage;
use smithay_client_toolkit::reexports::client::{protocol::wl_surface::WlSurface, Proxy};
use wayland_egl::WlEglSurface;

pub mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    pub use Gles2 as Gl;
}

pub struct Renderer {
    pub program: gl::types::GLuint,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    gl: gl::Gl,
}

// Macro that check the error code of the last OpenGL call and returns a Result.
macro_rules! gl_check {
    ($gl:expr, $desc:tt) => {{
        let error = $gl.GetError();
        if error != gl::NO_ERROR {
            let error_string = $gl.GetString(error);
            if error_string.is_null() {
                bail!("OpenGL error when {}: {}", $desc, error);
            } else {
                let error_string = CStr::from_ptr(error_string as _)
                    .to_string_lossy()
                    .into_owned();
                bail!("OpenGL error when {}: {} ({})", $desc, error, error_string);
            }
        }
    }};
}

pub struct EglContext {
    pub display: egl::Display,
    pub context: egl::Context,
    pub config: egl::Config,
    wl_egl_surface: WlEglSurface,
    surface: khronos_egl::Surface,
    // pub surface: egl::Surface,
    // pub wl_egl_surface: WlEglSurface,
}

impl EglContext {
    pub fn new(egl_display: egl::Display, wl_surface: &WlSurface) -> Self {
        const ATTRIBUTES: [i32; 7] = [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::NONE,
        ];

        let config = egl
            .choose_first_config(egl_display, &ATTRIBUTES)
            .expect("unable to choose an EGL configuration")
            .expect("no EGL configuration found");

        const CONTEXT_ATTRIBUTES: [i32; 5] = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            2,
            egl::NONE,
        ];

        let context = egl
            .create_context(egl_display, config, None, &CONTEXT_ATTRIBUTES)
            .expect("unable to create an EGL context");

        // First, create a small surface, we don't know the size of the output yet
        let wl_egl_surface = WlEglSurface::new(wl_surface.id(), 10, 10).unwrap();

        let surface = unsafe {
            egl.create_window_surface(
                egl_display,
                config,
                wl_egl_surface.ptr() as egl::NativeWindowType,
                None,
            )
            .expect("unable to create an EGL surface")
        };

        Self {
            display: egl_display,
            context,
            config,
            surface,
            wl_egl_surface,
        }
    }

    pub fn make_current(&self) -> Result<()> {
        Ok(egl
            .make_current(
                self.display,
                Some(self.surface),
                Some(self.surface),
                Some(self.context),
            )
            .with_context(|| "unable to make the context current")?)
    }

    // Swap the buffers of the surface
    pub fn swap_buffers(&self) -> Result<()> {
        Ok(egl
            .swap_buffers(self.display, self.surface)
            .with_context(|| "unable to post the surface content")?)
    }

    /// Resize the surface
    /// Resizing the surface means to destroy the previous one and then recreate it
    pub fn resize(&mut self, wl_surface: &WlSurface, width: i32, height: i32) {
        egl.destroy_surface(self.display, self.surface).unwrap();
        let wl_egl_surface = WlEglSurface::new(wl_surface.id(), width, height).unwrap();

        let surface = unsafe {
            egl.create_window_surface(
                self.display,
                self.config,
                wl_egl_surface.ptr() as egl::NativeWindowType,
                None,
            )
            .expect("unable to create an EGL surface")
        };

        self.surface = surface;
        self.wl_egl_surface = wl_egl_surface;
    }
}

impl Renderer {
    pub unsafe fn new() -> Result<Self> {
        let gl = gl::Gl::load_with(|name| {
            egl.get_proc_address(name).unwrap() as *const std::ffi::c_void
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
        let mut vao = 0;
        gl.GenVertexArrays(1, &mut vao);
        gl_check!(gl, "generating the vertex array");
        gl.BindVertexArray(vao);
        gl_check!(gl, "binding the vertex array");
        let mut vbo = 0;
        gl.GenBuffers(2, &mut vbo);
        gl_check!(gl, "generating the vbo buffer");
        gl.BindBuffer(gl::ARRAY_BUFFER, vbo);
        gl_check!(gl, "binding the vbo buffer");
        gl.BufferData(
            gl::ARRAY_BUFFER,
            (VERTEX_DATA.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
            VERTEX_DATA.as_ptr() as *const _,
            gl::STATIC_DRAW,
        );
        gl_check!(gl, "buffering the data");
        const POS_ATTRIB: i32 = 0;
        const TEX_ATTRIB: i32 = 1;
        gl.VertexAttribPointer(
            POS_ATTRIB as gl::types::GLuint,
            2,
            gl::FLOAT,
            0,
            4 * std::mem::size_of::<f32>() as gl::types::GLsizei,
            std::ptr::null(),
        );
        gl_check!(gl, "setting the position attribute for the vertex");
        gl.EnableVertexAttribArray(POS_ATTRIB as gl::types::GLuint);
        gl_check!(gl, "enabling the position attribute for the vertex");
        gl.VertexAttribPointer(
            TEX_ATTRIB as gl::types::GLuint,
            2,
            gl::FLOAT,
            0,
            4 * std::mem::size_of::<f32>() as gl::types::GLsizei,
            (2 * std::mem::size_of::<f32>()) as *const () as *const _,
        );
        gl_check!(gl, "setting the texture attribute for the vertex");
        gl.EnableVertexAttribArray(TEX_ATTRIB as gl::types::GLuint);
        gl_check!(gl, "enabling the texture attribute for the vertex");

        Ok(Self {
            program,
            vao,
            vbo,
            gl,
        })
    }

    pub fn check_error(&self, msg: &str) -> Result<()> {
        unsafe {
            gl_check!(self.gl, msg);
        }
        Ok(())
    }

    pub unsafe fn draw(&self, image: DynamicImage) -> Result<()> {
        // self.egl_context.make_current().context("while trying to draw the image")?;

        // self.resize(image.width().try_into().unwrap(), image.height().try_into().unwrap());

        let mut texture = 0;
        unsafe {
            self.gl.GenTextures(1, &mut texture);
            self.check_error("generating textures")?;
            self.gl.BindTexture(gl::TEXTURE_2D, texture);
            self.check_error("binding textures")?;
            self.gl.TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8.try_into().unwrap(),
                image.width().try_into().unwrap(),
                image.height().try_into().unwrap(),
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                image.as_bytes().as_ptr() as *const c_void,
                // buffer.as_ptr() as *const std::ffi::c_void,
            );
            self.check_error("defining the texture")?;
            self.gl.ActiveTexture(gl::TEXTURE0);
            self.check_error("enabling the texture")?;
            let loc = self
                .gl
                .GetUniformLocation(self.program, b"u_texture\0".as_ptr() as *const _);
            self.check_error("getting the uniform location")?;
            self.gl.Uniform1i(loc, 0);
            self.check_error("calling Uniform1i")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            self.check_error("defining the texture min filter")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            self.check_error("defining the texture mag filter")?;
            // Wait for the previous commands to finish before reading from the framebuffer.
            self.gl.Finish();
        }

        self.gl.UseProgram(self.program);
        self.check_error("calling UseProgram")?;
        self.gl.BindVertexArray(self.vao);
        self.check_error("binding the vertex array")?;
        self.gl.BindBuffer(gl::ARRAY_BUFFER, self.vbo);
        self.check_error("binding the buffer")?;

        self.gl.ClearColor(0.1, 0.1, 0.1, 0.9);
        self.check_error("calling ClearColor")?;
        self.gl.Clear(gl::COLOR_BUFFER_BIT);
        self.check_error("clearing the color buffer bit")?;
        self.gl.DrawArrays(gl::TRIANGLES, 0, 6);
        self.check_error("drawing the triangles")?;

        Ok(())
    }

    pub fn clear_after_draw(&self) -> Result<()> {
        unsafe {
            // Unbind the framebuffer and renderbuffer before deleting.
            self.gl.BindBuffer(gl::PIXEL_UNPACK_BUFFER, 0);
            self.check_error("unbinding the unpack buffer")?;
            self.gl.BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            self.check_error("unbinding the framebuffer")?;
            self.gl.BindRenderbuffer(gl::RENDERBUFFER, 0);
            self.check_error("unbinding the render buffer")?;
            self.gl.BindTexture(gl::TEXTURE_2D, 0);
            self.check_error("undinding the texture")?;
        }

        Ok(())
    }

    pub fn resize(&self, width: i32, height: i32) {
        unsafe {
            self.gl.Viewport(0, 0, width, height);
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
            self.gl.DeleteProgram(self.program);
            self.gl.DeleteBuffers(1, &self.vbo);
            self.gl.DeleteVertexArrays(1, &self.vao);
        }
    }
}

unsafe fn create_shader(
    gl: &gl::Gl,
    shader: gl::types::GLenum,
    source: &[u8],
) -> Result<gl::types::GLuint> {
    let shader = gl.CreateShader(shader);
    gl_check!(gl, "calling CreateShader");
    gl.ShaderSource(
        shader,
        1,
        [source.as_ptr().cast()].as_ptr(),
        std::ptr::null(),
    );
    gl_check!(gl, "calling Shadersource");
    gl.CompileShader(shader);
    gl_check!(gl, "calling CompileShader");

    let mut status: i32 = 0;
    gl.GetShaderiv(shader, gl::COMPILE_STATUS, &mut status as *mut _);
    gl_check!(gl, "calling GetShaderiv");
    if status == 0 {
        let mut max_length: i32 = 0;
        let mut length: i32 = 0;
        gl.GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut max_length as *mut _);
        gl_check!(gl, "calling GetShaderiv");
        let mut log: Vec<u8> = Vec::with_capacity(max_length as _);
        log.resize(max_length as _, 0);
        gl.GetShaderInfoLog(
            shader,
            max_length,
            &mut length as *mut _,
            log.as_mut_ptr() as _,
        );
        gl_check!(gl, "calling GetShaderInfoLog");
        let log = String::from_utf8(log).unwrap();
        Err(color_eyre::eyre::anyhow!(log))
    } else {
        Ok(shader)
    }
}

#[rustfmt::skip]
static VERTEX_DATA: [f32; 24] = [
    -1.0, -1.0,  0.0,  1.0,
    -1.0,  1.0,  0.0,  0.0,
     1.0,  1.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,
     1.0, -1.0,  1.0,  1.0,
    -1.0, -1.0,  0.0,  1.0,
];

const VERTEX_SHADER_SOURCE: &[u8] = b"
#version 100
precision mediump float;

attribute vec2 position;
attribute vec2 texcoord;

varying vec2 v_texcoord;

void main() {
    gl_Position = vec4(position, 1.0, 1.0);
    v_texcoord = texcoord;
}
\0";

const FRAGMENT_SHADER_SOURCE: &[u8] = b"
#version 100
precision mediump float;

uniform sampler2D u_texture;
varying vec2 v_texcoord;

void main() {
    gl_FragColor = texture2D(u_texture, v_texcoord);
}
\0";
