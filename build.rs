use cargo_lock::Lockfile;
use vergen::{BuildBuilder, Emitter, RustcBuilder};

fn main() -> anyhow::Result<()> {
    // Proto
    let mut config = prost_build::Config::new();
    let proto_file = "proto/event.proto";

    println!("cargo:rerun-if-changed={}", proto_file);

    config.boxed(".blockdaemon.solana.accountsdb_plugin_kafka.types.MessageWrapper");
    config.protoc_arg("--experimental_allow_proto3_optional");
    config.compile_protos(&[proto_file], &["proto/"])?;

    // Version metrics
    let _ = Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&RustcBuilder::all_rustc()?)?
        .emit();

    // vergen git version does not looks cool
    println!(
        "cargo:rustc-env=GIT_VERSION={}",
        git_version::git_version!()
    );

    // Extract Solana version
    let lockfile = Lockfile::load("./Cargo.lock")?;
    println!(
        "cargo:rustc-env=SOLANA_SDK_VERSION={}",
        get_pkg_version(&lockfile, "solana-sdk")
    );

    Ok(())
}

fn get_pkg_version(lockfile: &Lockfile, pkg_name: &str) -> String {
    lockfile
        .packages
        .iter()
        .filter(|pkg| pkg.name.as_str() == pkg_name)
        .map(|pkg| pkg.version.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
