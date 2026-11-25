use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest_dir.parent().unwrap();
    let zip_path = root.join("game").join("Parasol Stars (1992)(Ocean)[cr].zip");
    println!("cargo:rerun-if-changed={}", zip_path.display());
    println!("cargo:rerun-if-changed=../recomp/src/lifter.rs");
    println!("cargo:rerun-if-changed=../recomp/src/m68k.rs");

    let adf = recomp::zip_adf::load_adf_from_path(&zip_path)
        .expect("Failed to load Parasol Stars ADF from game/Parasol Stars (1992)(Ocean)[cr].zip");
    let main_bin = &adf[recomp::MAIN_DISK_OFFSET..recomp::MAIN_DISK_OFFSET + recomp::MAIN_LOAD_SIZE];

    // Extra statically discovered roots (updated as we verify full gameplay coverage)
    let extra_roots: &[u32] = &[];
    let cfg = recomp::lifter::build_cfg(main_bin, extra_roots);
    let code = recomp::lifter::emit_rust(&cfg);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("lifted.rs");
    fs::write(&out_path, code).expect("Failed to write lifted.rs");
}
