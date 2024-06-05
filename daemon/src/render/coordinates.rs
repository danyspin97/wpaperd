pub struct Coordinates {
    x_left: f32,
    x_right: f32,
    y_bottom: f32,
    y_top: f32,
}

impl Coordinates {
    pub const VEC_X_LEFT: f32 = -1.0;
    pub const VEC_X_RIGHT: f32 = 1.0;
    pub const VEC_Y_BOTTOM: f32 = 1.0;
    pub const VEC_Y_TOP: f32 = -1.0;

    pub const TEX_X_LEFT: f32 = 0.0;
    pub const TEX_X_RIGHT: f32 = 1.0;
    pub const TEX_Y_BOTTOM: f32 = 0.0;
    pub const TEX_Y_TOP: f32 = 1.0;

    pub fn new(x_left: f32, x_right: f32, y_bottom: f32, y_top: f32) -> Self {
        Self {
            x_left,
            x_right,
            y_bottom,
            y_top,
        }
    }

    pub const fn default_vec_coordinates() -> Self {
        Self {
            x_right: Self::VEC_X_RIGHT,
            x_left: Self::VEC_X_LEFT,
            y_bottom: Self::VEC_Y_BOTTOM,
            y_top: Self::VEC_Y_TOP,
        }
    }

    pub const fn default_texture_coordinates() -> Self {
        Self {
            x_right: Self::TEX_X_RIGHT,
            x_left: Self::TEX_X_LEFT,
            y_bottom: Self::TEX_Y_BOTTOM,
            y_top: Self::TEX_Y_TOP,
        }
    }
}

pub fn get_opengl_point_coordinates(
    vec_coordinates: Coordinates,
    tex_coordinates: Coordinates,
) -> [f32; 16] {
    [
        vec_coordinates.x_left, // top left start
        vec_coordinates.y_top,
        tex_coordinates.x_left,
        tex_coordinates.y_top,  // top left stop
        vec_coordinates.x_left, // bottom left start
        vec_coordinates.y_bottom,
        tex_coordinates.x_left,
        tex_coordinates.y_bottom, // bottom left stop
        vec_coordinates.x_right,  // bottom right start
        vec_coordinates.y_bottom,
        tex_coordinates.x_right,
        tex_coordinates.y_bottom, // bottom right stop
        vec_coordinates.x_right,  // top right start
        vec_coordinates.y_top,
        tex_coordinates.x_right,
        tex_coordinates.y_top, // top right // stop
    ]
}
