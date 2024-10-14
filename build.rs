use cargo_lock::Lockfile;

fn main() -> anyhow::Result<()> {
    // Proto
    let mut config = prost_build::Config::new();
    config.boxed(".blockdaemon.solana.accountsdb_plugin_kafka.types.MessageWrapper");
    config.protoc_arg("--experimental_allow_proto3_optional");
    config.compile_protos(&["proto/event.proto"], &["proto/"])?;

    // Version metrics
    vergen::Emitter::default()
        .add_instructions(&vergen::BuildBuilder::all_build()?)?
        .add_instructions(&vergen::RustcBuilder::all_rustc()?)?
        .emit()?;

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
