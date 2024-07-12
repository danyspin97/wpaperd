extern crate gl_generator;

use gl_generator::{Api, Fallbacks, Profile, Registry, StructGenerator};
use std::env;
use std::fs::File;
use std::io::Error;
use std::path::Path;

use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;

include!("src/opts.rs");

fn build_shell_completion(outdir: &Path) -> Result<(), Error> {
    let mut app = Opts::command();
    let shells = Shell::value_variants();

    for shell in shells {
        generate_to(*shell, &mut app, "wpaperd", outdir)?;
    }

    Ok(())
}

fn build_manpages(outdir: &Path) -> Result<(), Error> {
    let app = Opts::command();

    let file = Path::new(&outdir).join("wpaperd.1");
    let mut file = File::create(file)?;

    Man::new(app).render(&mut file)?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/opts.rs");

    let outdir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&outdir).ancestors().nth(3).unwrap();
    let comp_path = dest.join("completions");
    let man_path = dest.join("man");
    std::fs::create_dir_all(&comp_path)?;
    std::fs::create_dir_all(&man_path)?;

    build_shell_completion(&comp_path)?;
    build_manpages(&man_path)?;

    let mut file = File::create(Path::new(&outdir).join("gl_bindings.rs")).unwrap();

    Registry::new(
        Api::Gles2,
        (2, 0),
        Profile::Core,
        Fallbacks::All,
        ["GL_EXT_texture_border_clamp"],
    )
    .write_bindings(StructGenerator, &mut file)
    .unwrap();

    Ok(())
}
