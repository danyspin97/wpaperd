use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
use std::fs::File;
use std::io::Error;
use std::path::Path;

include!("src/config.rs");

fn build_shell_completion(outdir: &Path) -> Result<(), Error> {
    let mut app = Config::command();
    let shells = Shell::value_variants();

    for shell in shells {
        generate_to(*shell, &mut app, "wpaperd", &outdir)?;
    }

    Ok(())
}

fn build_manpages(outdir: &Path) -> Result<(), Error> {
    let app = Config::command();

    let file = Path::new(&outdir).join("wpaperd.1");
    let mut file = File::create(&file)?;

    Man::new(app).render(&mut file)?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src/config.rs");
    println!("cargo:rerun-if-changed=man");

    let comp_path = Path::new("completions");
    let man_path = Path::new("man");
    std::fs::create_dir_all(comp_path)?;
    std::fs::create_dir_all(man_path)?;

    build_shell_completion(comp_path)?;
    build_manpages(man_path)?;

    Ok(())
}
