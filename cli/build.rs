use std::env;
use std::fs::File;
use std::path::Path;
use std::io::Error;

use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;

include!("src/opts.rs");

fn build_shell_completion(outdir: &Path) -> Result<(), Error> {
    let mut app = Opts::command();
    let shells = Shell::value_variants();

    for shell in shells {
        generate_to(*shell, &mut app, "wpaperctl", outdir)?;
    }

    Ok(())
}

fn build_manpages(outdir: &Path) -> Result<(), Error> {
    let app = Opts::command();

    let file = Path::new(&outdir).join("wpaperctl.1");
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

    Ok(())
}

