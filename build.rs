use vergen_gix::{Build, Emitter, Gix, Rustc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = Build::builder().build_date(true).build();
    let rustc = Rustc::builder().semver(true).build();
    let git = Gix::builder().sha(true).dirty(false).build();
    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&rustc)?
        .add_instructions(&git)?
        .emit()?;
    Ok(())
}
