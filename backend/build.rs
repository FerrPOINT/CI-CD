use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(base_private_acl_witness)");
    let scripts = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../services-base/scripts");
    let postgres = scripts.join("platform_postgres.py");
    let acl = scripts.join("private_acl.py");
    println!("cargo:rerun-if-changed={}", postgres.display());
    println!("cargo:rerun-if-changed={}", acl.display());
    let source = fs::read_to_string(postgres).expect("pinned Base PostgreSQL helper required");
    assert!(
        !source.contains("scripts.private_acl") || acl.is_file(),
        "pinned Base PostgreSQL helper requires its private_acl dependency"
    );
    // Older exact SDK pins predate this dependency; keep their three-file packet.
    if acl.is_file() {
        println!("cargo:rustc-cfg=base_private_acl_witness");
    }
}
