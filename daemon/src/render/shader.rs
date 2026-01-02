use std::ffi::CStr;

use color_eyre::Result;

use crate::gl_check;

use super::gl;

pub unsafe fn create_shader(
    gl: &gl::Gl,
    shader: gl::types::GLenum,
    sources: &[*const std::ffi::c_char],
) -> Result<gl::types::GLuint> {
    let shader = gl.CreateShader(shader);
    gl_check!(gl, "calling CreateShader");
    gl.ShaderSource(
        shader,
        sources.len() as i32,
        sources.as_ptr().cast(),
        std::ptr::null(),
    );
    gl_check!(gl, "Failed to set the shader source");
    gl.CompileShader(shader);
    gl_check!(gl, "Failed to compile the shader");

    let mut status: i32 = 0;
    gl.GetShaderiv(shader, gl::COMPILE_STATUS, &mut status as *mut _);
    gl_check!(gl, "Failed to get the shader compile status");
    if status == 0 {
        let mut max_length: i32 = 0;
        let mut length: i32 = 0;
        gl.GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut max_length as *mut _);
        gl_check!(
            gl,
            "Failed to get the size of the information about the shader error"
        );
        let mut log: Vec<u8> = vec![0; max_length as _];
        gl.GetShaderInfoLog(
            shader,
            max_length,
            &mut length as *mut _,
            log.as_mut_ptr() as _,
        );
        gl_check!(gl, "Failed to get the information about the shader error");
        let res = String::from_utf8(log);
        match res {
            Ok(log) => Err(color_eyre::eyre::eyre!(log)),
            Err(err) => Err(color_eyre::eyre::eyre!(err)),
        }
    } else {
        Ok(shader)
    }
}

pub const VERTEX_SHADER_SOURCE: &CStr = c"
#version 310 es
precision mediump float;

layout (location = 0) in vec2 aPosition;
layout (location = 1) in vec2 aTexCoord;

layout (location = 10) uniform mat2 projection_matrix;

out vec2 v_texcoord;

void main() {
    gl_Position = vec4(aPosition* projection_matrix, 1.0, 1.0);
    v_texcoord = aTexCoord;
}";

pub const FRAGMENT_SHADER_SOURCE: &CStr = c"
#version 310 es
precision highp float;
out vec4 FragColor;

in vec2 v_texcoord;

layout (location = 2) uniform vec2 textureScale;
layout (location = 3) uniform vec2 prevTextureScale;
layout (location = 4) uniform sampler2D u_prev_texture;
layout (location = 5) uniform sampler2D u_texture;

uniform float progress;
uniform float ratio;
uniform float texture_offset;

vec4 transition(vec2);

vec4 getFromColor(vec2 uv) {
    uv = (uv - texture_offset) * prevTextureScale + (texture_offset);
    return texture(u_prev_texture, uv);
}

vec4 getToColor(vec2 uv) {
    uv = (uv - texture_offset) * textureScale + (texture_offset);
    return texture(u_texture, uv);
}

void main() {
    FragColor = transition(v_texcoord);
}";
