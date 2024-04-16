use std::ffi::CStr;

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};

use crate::gl_check;

use super::gl;

pub unsafe fn create_shader(
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
        let res = String::from_utf8(log);
        match res {
            Ok(log) => Err(color_eyre::eyre::anyhow!(log)),
            Err(err) => Err(color_eyre::eyre::anyhow!(err)),
        }
    } else {
        Ok(shader)
    }
}

pub const VERTEX_SHADER_SOURCE: &[u8] = b"
#version 320 es
precision mediump float;

layout (location = 0) in vec2 aPosition;
layout (location = 1) in vec2 aCurrentTexCoord;
layout (location = 2) in vec2 aOldTexCoord;

out vec2 v_old_texcoord;
out vec2 v_current_texcoord;

void main() {
    gl_Position = vec4(aPosition, 1.0, 1.0);
    v_current_texcoord = aCurrentTexCoord;
    v_old_texcoord = aOldTexCoord;
}
\0";

pub const FRAGMENT_SHADER_SOURCE: &[u8] = b"
#version 320 es
precision mediump float;
out vec4 FragColor;

in vec2 v_old_texcoord;
in vec2 v_current_texcoord;

layout(location = 0) uniform sampler2D u_old_texture;
layout(location = 1) uniform sampler2D u_current_texture;

layout(location = 2) uniform float u_progress;

void main() {
    FragColor = mix(texture(u_old_texture, v_old_texcoord), texture(u_current_texture, v_current_texcoord), u_progress);
}
\0";
