# Solana AccountsDB Plugin for Kafka
Context:

- geyser plugin nodes are responsible for sending account info updates to [our secondary infrastructure](https://www.notion.so/Solana-Account-Cache-via-Geyser-Plugin-d131d72fd0744dda86b7f9cb2c166d6a)
- geyser plugin library version need to be in sync with the solana binary version, therefore we’ll need to rebuild the geyser plugin library each time we upgrade solana binary

## BlockDaemon
- This is a fork of blockdaemon's geyser plugin for kafka
- Refer to the git repo for original documentation on configuration, etc 
  https://github.com/Blockdaemon/solana-accountsdb-plugin-kafka

## How to Build

Steps to prepare Geyser Plugin Library:
 
- The "intended version" listed below need to match the solana binary version in [alchemy-infrastructure](https://github.com/OMGWINNING/alchemy-infrastructure/blob/master/ansible/group_vars/solana_mainnet#L1)  
- Download the intended version of solana docker image from [docker hub](https://hub.docker.com/r/solanalabs/solana/tags)
- Update the `Cargo.toml` file in this repo to reflect the intended solana binary version. ([Sample Commit](https://github.com/OMGWINNING/solana-accountsdb-plugin-kafka/commit/81fc48d2ed61e8b900665d199e75de2890c6a150)) 
- Run the solana docker image locally with geyser plugin library path mapped to container path
- Bash into the solana docker container, go to the path of the geyser plugin library, and run following commands to install the dependencies

    ```bash
    apt-get update
    apt-get curl
    curl https://sh.rustup.rs -sSf | sh
    source $HOME/.cargo/env
    rustup component add rustfmt
    apt-get install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang cmake make libprotobuf-dev protobuf-compiler libsdl2-dev
    Cargo build --release
    ```

- If the Cargo build was successful, it would generate a `.so` file in ./target/release folder
- Add the version to the end of the `.so` file, i.e. `libsolana_accountsdb_plugin_kafka_v1.13.3.so`
- Update the file to **solana-geyser-plugin-lib** S3 bucket [https://s3.console.aws.amazon.com/s3/buckets/solana-geyser-plugin-lib?region=us-east-1&tab=objects](https://s3.console.aws.amazon.com/s3/buckets/solana-geyser-plugin-lib?region=us-east-1&tab=objects)
- Run AWX upgrade_nodes.yml file on geyser plugin nodes to update it
