mod coordinates;
mod egl_context;
mod renderer;
mod shader;
mod transition;
mod wallpaper;

use std::ffi::{c_void, CStr};

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use image::DynamicImage;

pub use egl_context::EglContext;
pub use renderer::Renderer;
pub use transition::Transition;

pub mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    pub use Gles2 as Gl;
}

// Macro that check the error code of the last OpenGL call and returns a Result.
#[macro_export]
macro_rules! gl_check {
    ($gl:expr, $desc:expr) => {{
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

fn initialize_objects(
    gl: &gl::Gl,
) -> Result<(gl::types::GLuint, gl::types::GLuint, gl::types::GLuint)> {
    unsafe {
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
        let vertex_data: Vec<f32> = vec![0.0; 24 as _];
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
        // We load the elements array buffer once, it's the same for each wallpaper
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

        Ok((vao, vbo, eab))
    }
}

fn load_texture(gl: &gl::Gl, image: DynamicImage) -> Result<gl::types::GLuint> {
    Ok(unsafe {
        let mut texture = 0;
        gl.GenTextures(1, &mut texture);
        gl_check!(gl, "generating textures");
        gl.ActiveTexture(gl::TEXTURE0);
        gl_check!(gl, "activating textures");
        gl.BindTexture(gl::TEXTURE_2D, texture);
        gl_check!(gl, "binding textures");
        gl.TexImage2D(
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
        gl_check!(gl, "defining the texture");
        gl.GenerateMipmap(gl::TEXTURE_2D);
        gl_check!(gl, "generating the mipmap");
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl_check!(gl, "defining the texture min filter");
        gl.TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl_check!(gl, "defining the texture mag filter");

        texture
    })
}
