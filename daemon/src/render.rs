use std::{
    ffi::{CStr, CString},
    ops::Deref,
};

use egl::API as egl;
use glutin::prelude::*;

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

impl Renderer {
    pub fn new() -> Self {
        unsafe {
            let gl = gl::Gl::load_with(|name| {
                egl.get_proc_address(name).unwrap() as *const std::ffi::c_void
            });

            let vertex_shader =
                create_shader(&gl, gl::VERTEX_SHADER, VERTEX_SHADER_SOURCE).expect("vertex_shader");
            println!("{}", gl.GetError());
            let fragment_shader = create_shader(&gl, gl::FRAGMENT_SHADER, FRAGMENT_SHADER_SOURCE)
                .expect("fragment_shader");
            println!("{}", gl.GetError());

            let program = gl.CreateProgram();
            println!("{}", gl.GetError());

            gl.AttachShader(program, vertex_shader);
            println!("{}", gl.GetError());
            gl.AttachShader(program, fragment_shader);
            println!("{}", gl.GetError());

            gl.LinkProgram(program);
            println!("{}", gl.GetError());

            gl.UseProgram(program);
            println!("{}", gl.GetError());

            gl.DeleteShader(vertex_shader);
            println!("delete {}", gl.GetError());
            gl.DeleteShader(fragment_shader);
            println!("{}", gl.GetError());

            let mut vao = 0;
            gl.GenVertexArrays(1, &mut vao);
            println!("{}", gl.GetError());
            gl.BindVertexArray(vao);
            println!("{}", gl.GetError());

            let mut vbo = 0;
            gl.GenBuffers(2, &mut vbo);
            println!("{}", gl.GetError());
            gl.BindBuffer(gl::ARRAY_BUFFER, vbo);
            println!("{}", gl.GetError());
            gl.BufferData(
                gl::ARRAY_BUFFER,
                (VERTEX_DATA.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
                VERTEX_DATA.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );
            println!("{}", gl.GetError());
            // gl.BindBuffer(gl::ARRAY_BUFFER, vbo[1]);
            // gl.BufferData(
            //     gl::ARRAY_BUFFER,
            //     (VERTEX_DATA2.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
            //     VERTEX_DATA2.as_ptr() as *const _,
            //     gl::STATIC_DRAW,
            // );

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
            println!("{}", gl.GetError());
            // gl.EnableVertexAttribArray(POS_ATTRIB as gl::types::GLuint);
            println!("{}", gl.GetError());

            gl.VertexAttribPointer(
                TEX_ATTRIB as gl::types::GLuint,
                2,
                gl::FLOAT,
                0,
                4 * std::mem::size_of::<f32>() as gl::types::GLsizei,
                (2 * std::mem::size_of::<f32>()) as *const () as *const _,
            );
            println!("vertexattrib tex {}", gl.GetError());
            // gl.EnableVertexAttribArray(TEX_ATTRIB as gl::types::GLuint);
            println!("tex_attrib {}", gl.GetError());

            Self {
                program,
                vao,
                vbo,
                gl,
            }
        }
    }

    pub fn draw(&self) {
        unsafe {
            self.gl.UseProgram(self.program);

            self.gl.BindVertexArray(self.vao);
            println!("vao: {}", self.vao);
            println!("bindvertex {}", self.GetError());
            self.gl.BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            println!("{}", self.GetError());

            self.gl.ClearColor(0.1, 0.1, 0.1, 0.9);
            println!("{}", self.GetError());
            self.gl.Clear(gl::COLOR_BUFFER_BIT);
            println!("{}", self.GetError());
            self.gl.DrawArrays(gl::TRIANGLES, 0, 6);
            println!("{}", self.GetError());
        }
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
) -> Result<gl::types::GLuint, String> {
    let shader = gl.CreateShader(shader);
    gl.ShaderSource(
        shader,
        1,
        [source.as_ptr().cast()].as_ptr(),
        std::ptr::null(),
    );
    gl.CompileShader(shader);

    let mut status: i32 = 0;
    gl.GetShaderiv(shader, gl::COMPILE_STATUS, &mut status as *mut _);
    if status == 0 {
        let mut max_length: i32 = 0;
        let mut length: i32 = 0;
        gl.GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut max_length as *mut _);
        let mut log: Vec<u8> = Vec::with_capacity(max_length as _);
        log.resize(max_length as _, 0);
        gl.GetShaderInfoLog(
            shader,
            max_length,
            &mut length as *mut _,
            log.as_mut_ptr() as _,
        );
        let log = String::from_utf8(log).unwrap();
        Err(log)
    } else {
        Ok(shader)
    }
}

fn get_gl_string(gl: &gl::Gl, variant: gl::types::GLenum) -> Option<&'static CStr> {
    unsafe {
        let s = gl.GetString(variant);
        (!s.is_null()).then(|| CStr::from_ptr(s.cast()))
    }
}

#[rustfmt::skip]
static VERTEX_DATA: [f32; 24] = [
    -1.0, -1.0,  0.0,  0.0,
    -1.0,  1.0,  1.0,  0.0,
     1.0,  1.0,  0.0,  1.0,
     1.0,  1.0,  0.0,  0.0,
     1.0, -1.0,  1.0,  0.0,
    -1.0, -1.0,  0.0,  1.0,
];

const VERTEX_SHADER_SOURCE: &[u8] = b"
#version 100
precision mediump float;

attribute vec2 position;
attribute vec2 texcoord;

varying vec2 v_texcoord;

void main() {
    gl_Position = vec4(position, 0.0, 1.0);
    v_texcoord = texcoord;
}
\0";

// const VERTEX_SHADER_SOURCE: &[u8] = b"
// #version 320 es
// precision mediump float;
// layout(location = 0) in vec2 position;
// layout(location = 1) in vec2 texCoord;

// out vec2 v_texCoord;

// void main(){
//     gl_Position = vec4(position, 0.0, 1.0);
//     v_texCoord = texCoord;
// }// \0";

// const FRAGMENT_SHADER_SOURCE: &[u8] = b"
// precision highp float;

// uniform sampler2D u_texture;
// varying vec2 v_texCoord;

// void main(void){
//     gl_FragColor = texture2D(u_texture, v_texCoord);;
// }// \0";
const FRAGMENT_SHADER_SOURCE: &[u8] = b"
#version 100
precision mediump float;

uniform sampler2D u_texture;
varying vec2 v_texcoord;

void main() {
    // gl_FragColor = vec4(v_color, 1.0);
    gl_FragColor = texture2D(u_texture, v_texcoord);;
}
\0";
