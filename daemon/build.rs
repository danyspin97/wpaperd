extern crate gl_generator;

use gl_generator::{Api, Fallbacks, Profile, Registry, StructGenerator};
use std::env;
use std::fs::File;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let dest = env::var("OUT_DIR").unwrap();
    let mut file = File::create(&Path::new(&dest).join("gl_bindings.rs")).unwrap();

    Registry::new(
        Api::Gles2,
        (3, 2),
        Profile::Core,
        Fallbacks::All,
        ["GL_OES_mapbuffer"],
    )
    .write_bindings(StructGenerator, &mut file)
    .unwrap();
}
