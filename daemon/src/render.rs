use std::{
    cell::RefCell,
    ffi::{c_void, CStr},
    ops::Deref,
    rc::Rc,
};

use color_eyre::{
    eyre::{bail, ensure, Context},
    Result,
};
use egl::API as egl;
use image::DynamicImage;
use smithay_client_toolkit::reexports::client::{protocol::wl_surface::WlSurface, Proxy};
use wayland_egl::WlEglSurface;

use crate::{surface::DisplayInfo, wallpaper_info::BackgroundMode};

pub mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    pub use Gles2 as Gl;
}

pub struct Renderer {
    pub program: gl::types::GLuint,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    eab: gl::types::GLuint,
    gl: gl::Gl,
    // reverse: false
    // old gl texture0
    // new gl texture1
    // reverse: true
    // new gl texture0
    // old texture1
    reverse: bool,
    // milliseconds time for the animation
    animation_time: u32,
    pub time_started: u32,
    texture1: gl::types::GLuint,
    texture2: gl::types::GLuint,
    display_info: Rc<RefCell<DisplayInfo>>,
}

// Macro that check the error code of the last OpenGL call and returns a Result.
macro_rules! gl_check {
    ($gl:expr, $desc:tt) => {{
        let error = $gl.GetError();
        if error != gl::NO_ERROR {
            let error_string = $gl.GetString(error);
            ensure!(
                !error_string.is_null(),
                "OpenGL error when {}: {}",
                $desc,
                error
            );

            let error_string = CStr::from_ptr(error_string as _)
                .to_string_lossy()
                .into_owned();
            bail!("OpenGL error when {}: {} ({})", $desc, error, error_string);
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
        egl.make_current(
            self.display,
            Some(self.surface),
            Some(self.surface),
            Some(self.context),
        )
        .with_context(|| "unable to make the context current")
    }

    // Swap the buffers of the surface
    pub fn swap_buffers(&self) -> Result<()> {
        egl.swap_buffers(self.display, self.surface)
            .with_context(|| "unable to post the surface content")
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
    pub unsafe fn new(image: DynamicImage, display_info: Rc<RefCell<DisplayInfo>>) -> Result<Self> {
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
        gl.GenBuffers(1, &mut vbo);
        gl_check!(gl, "generating the vbo buffer");
        gl.BindBuffer(gl::ARRAY_BUFFER, vbo);
        gl_check!(gl, "binding the vbo buffer");
        let vertex_data: Vec<f32> = vec![0.0; 16 as _];
        gl.BufferData(
            gl::ARRAY_BUFFER,
            (vertex_data.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
            vertex_data.as_ptr() as *const _,
            gl::STATIC_DRAW,
        );
        gl_check!(gl, "buffering the data");

        let mut eab = 0;
        gl.GenBuffers(1, &mut eab);
        gl_check!(gl, "generating the eab buffer");
        gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, eab);
        gl_check!(gl, "binding the eab buffer");
        const INDICES: [gl::types::GLuint; 6] = [0, 1, 2, 2, 3, 0];
        gl.BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            (INDICES.len() * std::mem::size_of::<gl::types::GLuint>()) as gl::types::GLsizeiptr,
            INDICES.as_ptr() as *const _,
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

        gl.UseProgram(program);
        gl_check!(gl, "calling UseProgram");
        gl.BindVertexArray(vao);
        gl_check!(gl, "binding the vertex array");
        gl.BindBuffer(gl::ARRAY_BUFFER, vbo);
        gl_check!(gl, "binding the buffer");

        let mut renderer = Self {
            program,
            vao,
            vbo,
            eab,
            gl,
            reverse: true,
            time_started: 0,
            animation_time: 300,
            texture1: 0,
            texture2: 0,
            display_info,
        };

        renderer.load_texture(image, BackgroundMode::Stretch)?;

        Ok(renderer)
    }

    pub fn check_error(&self, msg: &str) -> Result<()> {
        unsafe {
            gl_check!(self.gl, msg);
        }
        Ok(())
    }

    pub unsafe fn draw(&self, time: u32) -> Result<()> {
        self.gl.Clear(gl::COLOR_BUFFER_BIT);
        self.check_error("clearing the screen")?;

        let elapsed = time - self.time_started;
        let mut progress = (elapsed as f32 / self.animation_time as f32).min(1.0);
        if self.reverse {
            progress = 1.0 - progress;
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

        Ok(())
    }

    pub fn load_texture(&mut self, image: DynamicImage, mode: BackgroundMode) -> Result<()> {
        let mut texture = 0;
        unsafe {
            self.gl.GenTextures(1, &mut texture);
            self.check_error("generating textures")?;
            self.gl.ActiveTexture(if self.reverse {
                gl::TEXTURE0
            } else {
                gl::TEXTURE1
            });
            self.check_error("activating textures")?;
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
            );
            self.check_error("defining the texture")?;
            self.gl.GenerateMipmap(gl::TEXTURE_2D);
            self.check_error("generating the mipmap")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            self.check_error("defining the texture min filter")?;
            self.gl
                .TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            self.check_error("defining the texture mag filter")?;
            // We assume that the we still have an active texture
            if self.reverse {
                self.gl.Uniform1i(0, 0);
            } else {
                self.gl.Uniform1i(1, 1);
            }
            self.reverse = !self.reverse;

            // Delete the old texture and update order
            std::mem::swap(&mut self.texture1, &mut self.texture2);
            self.gl.DeleteTextures(1, &self.texture2);
            self.check_error("deleting the texture")?;
            self.texture2 = texture;

            let vertex = self.generate_vertex_data(mode, image.width(), image.height());
            // Update the vertex buffer
            self.gl.BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (vertex.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
                vertex.as_ptr() as *const _,
            );
            self.check_error("buffering the data")?;
        }

        Ok(())
    }

    pub fn start_animation(&mut self, time: u32) {
        self.time_started = time;
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

    pub(crate) fn is_drawing_animation(&self, time: u32) -> bool {
        time < (self.time_started + self.animation_time)
    }

    fn generate_vertex_data(
        &self,
        mode: BackgroundMode,
        image_width: u32,
        image_height: u32,
    ) -> [f32; 16] {
        const VEC_X_LEFT: f32 = -1.0;
        const VEC_X_RIGHT: f32 = 1.0;
        const VEC_Y_BOTTOM: f32 = 1.0;
        const VEC_Y_TOP: f32 = -1.0;

        const TEX_X_LEFT: f32 = 0.0;
        const TEX_X_RIGHT: f32 = 1.0;
        const TEX_Y_BOTTOM: f32 = 0.0;
        const TEX_Y_TOP: f32 = 1.0;

        // adjusted_width and adjusted_height returns the rotated sizes in case
        // the display is rotated. However, openGL is drawing in the same orientation
        // as our display (i.e. we don't apply any transform here)
        // We still need the scale
        let display_width = self.display_info.borrow().scaled_width();
        let display_height = self.display_info.borrow().scaled_height();

        let (
            vec_x_left,
            vec_x_right,
            vec_y_bottom,
            vec_y_top,
            tex_x_left,
            tex_x_right,
            tex_y_bottom,
            tex_y_top,
        ) = match mode {
            BackgroundMode::Stretch => (
                VEC_X_LEFT,
                VEC_X_RIGHT,
                VEC_Y_BOTTOM,
                VEC_Y_TOP,
                TEX_X_LEFT,
                TEX_X_RIGHT,
                TEX_Y_BOTTOM,
                TEX_Y_TOP,
            ),
            BackgroundMode::Fill => {
                let display_ratio = display_width as f32 / display_height as f32;
                let image_ratio = image_width as f32 / image_height as f32;
                if display_ratio == image_ratio {
                    (
                        VEC_X_LEFT,
                        VEC_X_RIGHT,
                        VEC_Y_BOTTOM,
                        VEC_Y_TOP,
                        TEX_X_LEFT,
                        TEX_X_RIGHT,
                        TEX_Y_BOTTOM,
                        TEX_Y_TOP,
                    )
                } else if display_ratio > image_ratio {
                    // Same as width calculation below , but with inverted parameters
                    // This is the expanded expression
                    // adjusted_height = image_width as f32 / display_ratio;
                    // y = (1.0 - image_height as f32 / adjusted_height) / 2.0;
                    // We can simplify by just doing display_ration / image_ratio
                    let y = (1.0 - display_ratio / image_ratio) / 2.0;
                    (
                        VEC_X_LEFT,
                        VEC_X_RIGHT,
                        VEC_Y_BOTTOM,
                        VEC_Y_TOP,
                        TEX_X_LEFT,
                        TEX_X_RIGHT,
                        TEX_Y_BOTTOM - y,
                        TEX_Y_TOP + y,
                    )
                } else {
                    // Calculte the adjusted width, i.e. the width that the image should have to
                    // have the same ratio as the display
                    // adjusted_width = image_height as f32 * display_ratio;
                    // Calculate the ratio between the adjusted_width and the image_width
                    // x = (1.0 - adjusted_width / image_width as f32) / 2.0;
                    // Simplify the expression and do the same as above
                    let x = (1.0 - display_ratio / image_ratio) / 2.0;
                    (
                        VEC_X_LEFT,
                        VEC_X_RIGHT,
                        VEC_Y_BOTTOM,
                        VEC_Y_TOP,
                        TEX_X_LEFT + x,
                        TEX_X_RIGHT - x,
                        TEX_Y_BOTTOM,
                        TEX_Y_TOP,
                    )
                }
            }
            BackgroundMode::Fit => {
                let display_ratio = display_width as f32 / display_height as f32;
                let image_ratio = image_width as f32 / image_height as f32;
                if display_ratio == image_ratio {
                    (
                        VEC_X_LEFT,
                        VEC_X_RIGHT,
                        VEC_Y_BOTTOM,
                        VEC_Y_TOP,
                        TEX_X_LEFT,
                        TEX_X_RIGHT,
                        TEX_Y_BOTTOM,
                        TEX_Y_TOP,
                    )
                } else if display_ratio > image_ratio {
                    let x = image_ratio / display_ratio;
                    (
                        -x,
                        x,
                        VEC_Y_BOTTOM,
                        VEC_Y_TOP,
                        TEX_X_LEFT,
                        TEX_X_RIGHT,
                        TEX_Y_BOTTOM,
                        TEX_Y_TOP,
                    )
                } else {
                    let y = 1.0 - display_ratio / image_ratio;
                    (
                        VEC_X_LEFT,
                        VEC_X_RIGHT,
                        VEC_Y_BOTTOM - y,
                        VEC_Y_TOP + y,
                        TEX_X_LEFT,
                        TEX_X_RIGHT,
                        TEX_Y_BOTTOM,
                        TEX_Y_TOP,
                    )
                }
            }
            BackgroundMode::Tile => {
                // Tile using the original image size
                let x = display_width as f32 / image_width as f32;
                let y = display_height as f32 / image_height as f32;
                (
                    VEC_X_LEFT,
                    VEC_X_RIGHT,
                    VEC_Y_BOTTOM,
                    VEC_Y_TOP,
                    TEX_X_LEFT,
                    x,
                    TEX_Y_BOTTOM,
                    y,
                )
            }
        };

        #[rustfmt::skip]
        let vertex_data = [
            vec_x_left,  vec_y_top,    tex_x_left,  tex_y_top,    // top left
            vec_x_left,  vec_y_bottom, tex_x_left,  tex_y_bottom, // bottom left
            vec_x_right, vec_y_bottom, tex_x_right, tex_y_bottom, // bottom right
            vec_x_right, vec_y_top,    tex_x_right, tex_y_top,    // top right
        ];
        vertex_data
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
            self.gl.DeleteBuffers(1, &self.eab);
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
        let mut log: Vec<u8> = vec![0; max_length as _];
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

const VERTEX_SHADER_SOURCE: &[u8] = b"
#version 320 es
precision mediump float;

layout (location = 0) in vec2 aPosition;
layout (location = 1) in vec2 aTexCoord;

out vec2 v_texcoord;

void main() {
    gl_Position = vec4(aPosition, 1.0, 1.0);
    v_texcoord = aTexCoord;
}
\0";

const FRAGMENT_SHADER_SOURCE: &[u8] = b"
#version 320 es
precision mediump float;
out vec4 FragColor;

in vec2 v_texcoord;

layout(location = 0) uniform sampler2D u_texture0;
layout(location = 1) uniform sampler2D u_texture1;

layout(location = 2) uniform float u_progress;

void main() {
    FragColor = mix(texture(u_texture1, v_texcoord), texture(u_texture0, v_texcoord), u_progress);

}
\0";
