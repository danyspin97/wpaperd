use std::ffi::CStr;

use color_eyre::{
    eyre::{bail, ensure},
    Result,
};
use serde::Deserialize;
//use wpaperd_transitions_proc_macro::Transitions;

use crate::gl_check;

use super::gl;

type UniformCallback = dyn Fn(&gl::Gl, gl::types::GLuint) -> Result<()>;

macro_rules! include_cstr {
    ( $path:expr $(,)? ) => {{
        // Use a constant to force the verification to run at compile time.
        const VALUE: &'static ::core::ffi::CStr = match ::core::ffi::CStr::from_bytes_with_nul(
            concat!(include_str!($path), "\0").as_bytes(),
        ) {
            Ok(value) => value,
            Err(_) => panic!(concat!("interior NUL byte(s) in `", $path, "`")),
        };
        VALUE
    }};
}

use format_bytes::format_bytes;

trait UniformSetter {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint);
}

impl UniformSetter for bool {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform1i(loc, (*self).into());
        }
    }
}

impl UniformSetter for i32 {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform1i(loc, *self);
        }
    }
}

impl UniformSetter for f32 {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        gl.Uniform1f(loc, *self);
    }
}

impl UniformSetter for [i32; 2] {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform2iv(loc, 1, self.as_ptr());
        }
    }
}

impl UniformSetter for [u32; 2] {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform2uiv(loc, 1, self.as_ptr());
        }
    }
}

impl UniformSetter for [f32; 2] {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform2fv(loc, 1, self.as_ptr());
        }
    }
}

impl UniformSetter for [f32; 3] {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform3fv(loc, 1, self.as_ptr());
        }
    }
}

impl UniformSetter for [f32; 4] {
    unsafe fn set_uniform(&self, gl: &gl::Gl, loc: gl::types::GLint) {
        unsafe {
            gl.Uniform4fv(loc, 1, self.as_ptr());
        }
    }
}

macro_rules! transition_shader {
    ($enum:ident { $($variant:ident { $($field_name:ident: $field_ty:ty = ($glsl_name:literal, $default_value:expr)),* } => $default_time:expr),* }) => {
        #[derive(Deserialize, Clone, Debug, PartialEq)]
        #[serde(rename_all = "kebab-case", rename_all_fields = "kebab-case", deny_unknown_fields)]
        pub enum $enum {
            $($variant { $($field_name: Option<$field_ty>),* }),*
        }

        impl $enum {
            pub fn shader(self) -> (Box<UniformCallback>, &'static CStr) {
                match self {
                    //$($enum::$variant => (
                    //    Box::new(|_, _| Ok(())),
                    //    include_cstr!(concat!("shaders/", stringify!($variant), ".glsl")),
                    //),)*
                    $($enum::$variant { $($field_name),* } => (
                        #[allow(unused)]
                        Box::new(move |gl, program| {
                            $(
                                unsafe {
                                    let loc = gl.GetUniformLocation(program, format_bytes!(b"{}\0", $glsl_name.as_bytes()).as_ptr() as *const _);
                                    gl_check!(gl, format!("getting the uniform location for {}", $glsl_name));
                                    ensure!(loc >= 0, "uniform {} cannot be found", $glsl_name);
                                    $field_name.unwrap_or($default_value).set_uniform(gl, loc);
                                    gl_check!(gl, format!("calling Uniform on {}", $glsl_name));
                                }
                            )*
                            Ok(())
                        }),
                        include_cstr!(concat!("shaders/", stringify!($variant), ".glsl"))
                    ),)*
                }
            }

            pub const fn default_transition_time(&self) -> u32 {
                match self {
                    $($enum::$variant { .. } => $default_time,)*
                }
            }
        }
    };
}

transition_shader! {
    Transition {
        BookFlip{} => 2000,
        Bounce {
            shadow_colour: [f32; 4] = ("shadow_colour", [0.0, 0.0, 0.0, 0.6]),
            shadow_height: f32 = ("shadow_height", 0.075),
            bounces: f32 = ("bounces", 3.0)
        } => 4000,
        BowTieHorizontal{} => 1500,
        BowTieVertical{} => 1500,
        ButterflyWaveScrawler {
            amplitude: f32 = ("amplitude", 1.0),
            waves: f32 = ("waves", 30.0),
            color_separation: f32 = ("colorSeparation", 0.3)
        } => 2000,
        Circle{} => 3000,
        CircleCrop{
            bgcolor: [f32; 4] = ("bgcolor", [0.0, 0.0, 0.0, 1.0])
        } => 3000,
        CircleOpen {
            smoothness: f32 = ("smoothness", 0.3),
            opening: bool = ("opening", true)
        } => 1500,
        ColourDistance { power: f32 = ("power", 5.0) } => 2000,
        CrossWarp{} => 1000,
        CrossZoom { strength: f32 = ("strength", 0.4) } => 2000,
        Directional { direction: [f32; 2] = ("direction", [0.0, 1.0]) } => 1000,
        DirectionalScaled {
            direction: [f32; 2] = ("direction", [0.0, 1.0]),
            scale: f32 = ("scale", 0.7)
        } => 1000,
        DirectionalWipe {
            direction: [f32; 2] = ("direction", [1.0, -1.0]),
            smoothness: f32 = ("smoothness", 0.5)
        } => 1000,
        Dissolve {
            line_width: f32 = ("uLineWidth", 0.1),
            spread_clr: [f32; 3] = ("uSpreadClr", [1.0, 0.0, 0.0]),
            hot_clr: [f32; 3] = ("uHotClr", [0.9, 0.9, 0.2]),
            pow: f32 = ("uPow", 5.0),
            intensity: f32 = ("uIntensity", 1.0)
        } => 1000,
        Doom {
            bars: i32 = ("bars", 30),
            amplitude: f32 = ("amplitude", 2.0),
            noise: f32 = ("noise", 0.1),
            frequency: f32 = ("frequency", 0.5),
            drip_scale: f32 = ("dripScale", 0.5)
        } => 2000,
        Dreamy{} => 1500,
        DreamyZoom{
            rotation: f32 = ("rotation", 6.0),
            scale: f32 = ("scale", 1.2)
        } => 1500,
        Edge{
            thickness: f32 = ("edge_thickness", 0.001),
            brightness: f32 = ("edge_brightness", 8.0)
        } => 1500,
        Fade{} => 300,
        FilmBurn { seed: f32 = ("Seed", 2.31) } => 2000,
        GlitchDisplace{} => 1500,
        GlitchMemories{} => 1500,
        GridFlip {
            size: [i32; 2] = ("size", [4, 4]),
            pause: f32 = ("pause", 0.1),
            divider_width: f32 = ("dividerWidth", 0.05),
            bgcolor: [f32; 4] = ("bgcolor", [0.0, 0.0, 0.0, 1.0]),
           randomness: f32 = ("randomness", 0.1)
        } => 1500,
        Hexagonalize {
            steps: i32 = ("steps", 50),
            horizontal_hexagons: f32 = ("horizontalHexagons", 20.0)
        } => 2000,
        HorizontalClose{} => 2000,
        HorizontalOpen{} => 2000,
        InvertedPageCurl{} => 2000,
        LeftRight{} => 2000,
        LinearBlur { intensity: f32 = ("intensity", 0.1) } => 800,
        Mosaic{
            endx: i32 = ("endx", 2),
            endy: i32 = ("endy", -1)
        } => 2000,
        Overexposure{} => 2000,
        Pixelize {
            squares_min: [i32; 2] = ("squaresMin", [20, 20]),
            steps: i32 = ("steps", 50)
        } => 1500,
        PolkaDotsCurtain {
            dots: f32 = ("dots", 20.0),
            center: [f32; 2] = ("center", [0.0, 0.0])
        } => 2000,
        Radial { smoothness: f32 = ("smoothness", 1.0) } => 1500,
        Rectangle { bgcolor: [f32; 4] = ("bgcolor", [0.0, 0.0, 0.0, 1.0]) } => 2000,
        Ripple {
            amplitude: f32 = ("amplitude", 100.0),
            speed: f32 = ("speed", 50.0)
        } => 1500,
        Rolls {
            rolls_type: i32 = ("type", 0),
            rot_down: bool = ("RotDown", false)
        } => 2000,
        RotateScaleFade {
            center: [f32; 2] = ("center", [0.5, 0.5]),
            rotations: f32 = ("rotations", 1.0),
            scale: f32 = ("scale", 8.0),
            back_color: [f32; 4] = ("backColor", [0.15, 0.15, 0.15, 1.0])
        } => 1500
    }
}
