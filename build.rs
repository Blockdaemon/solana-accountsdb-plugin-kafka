use std::io::Result;

fn main() -> Result<()> {
    let mut config = prost_build::Config::new();
    config.boxed(".blockdaemon.solana.accountsdb_plugin_kafka.types.MessageWrapper");
    config.compile_protos(&["proto/event.proto"], &["proto/"])?;
    Ok(())
}
