# Solana AccountsDB Plugin for Kafka

Kafka publisher for use with Solana's [plugin framework](https://docs.solana.com/developing/plugins/geyser-plugins).

## Installation

### Binary releases

Find binary releases at: https://github.com/Blockdaemon/solana-accountsdb-plugin-kafka/releases

### Building from source

#### Prerequisites

You will need version 3.12 or later of the protobuf compiler `protoc` installed.

#### Build

```shell
cargo build --release
```

- Linux: `./target/release/libsolana_accountsdb_plugin_kafka.so`
- macOS: `./target/release/libsolana_accountsdb_plugin_kafka.dylib`

**Important:** Solana's plugin interface requires the build environment of the Solana validator and this plugin to be **identical**.

This includes the Solana version and Rust compiler version.
Loading a plugin targeting wrong versions will result in memory corruption and crashes.

## Config

Config is specified via the plugin's JSON config file.

### Example Config

```json
{
  "libpath": "target/release/libsolana_accountsdb_plugin_kafka.so",
  "kafka": {
    "bootstrap.servers": "localhost:9092",
    "request.required.acks": "1",
    "message.timeout.ms": "30000",
    "compression.type": "lz4",
    "partitioner": "murmur2_random"
  },
  "shutdown_timeout_ms": 30000,
  "update_account_topic": "solana.testnet.account_updates",
  "slot_status_topic": "solana.testnet.slot_status",
  "transaction_topic": "solana.testnet.transactions",
  "publish_all_accounts": false,
  "wrap_messages": false,
  "program_ignores": [
    "Sysvar1111111111111111111111111111111111111",
    "Vote111111111111111111111111111111111111111"
  ]
}
```

### Reference

- `libpath`: Path to Kafka plugin
- `kafka`: [`librdkafka` config options](https://github.com/edenhill/librdkafka/blob/master/CONFIGURATION.md).
  This plugin overrides the defaults as seen in the example config.
- `shutdown_timeout_ms`: Time the plugin is given to flush out all messages to Kafka upon exit request.
- `update_account_topic`: Topic name of account updates. Omit to disable.
- `slot_status_topic`: Topic name of slot status update. Omit to disable.
- `publish_all_accounts`: Publish all accounts on startup. Omit to disable.
- `wrap_messages`: Wrap all messages in a unified wrapper object. Omit to disable (see Message Wrapping below).
- `program_ignores`: Account addresses to ignore (see Filtering below).

### Message Keys

The message types are keyed as follows:
- **Account update:** account address (public key)
- **Slot status:** slot number
- **Transaction notification:** transaction signature

### Filtering

If `program_ignores` are specified, then these addresses will be filtered out of the account updates
and transaction notifications.  More specifically, account update messages for these accounts will not be emitted,
and transaction notifications for any transaction involving these accounts will not be emitted.

### Message Wrapping

In some cases it may be desirable to send multiple types of messages to the same topic,
for instance to preserve relative order.  In this case it is helpful if all messages conform to a single schema.
Setting `wrap_messages` to true will wrap all three message types in a uniform wrapper object so that they
conform to a single schema.

Note that if `wrap_messages` is true, in order to avoid key collision, the message keys are prefixed with a single byte,
which is dependent on the type of the message being wrapped.  Account update message keys are prefixed with
65 (A), slot status keys with 83 (S), and transaction keys with 84 (T).

## Buffering

The Kafka producer acts strictly non-blocking to allow the Solana validator to sync without much induced lag.
This means incoming events from the Solana validator will get buffered and published asynchronously.

When the publishing buffer is exhausted any additional events will get dropped.
This can happen when Kafka brokers are too slow or the connection to Kafka fails.
Therefor it is crucial to choose a sufficiently large buffer.

The buffer size can be controlled using `librdkafka` config options, including:
- `queue.buffering.max.messages`: Maximum number of messages allowed on the producer queue.
- `queue.buffering.max.kbytes`: Maximum total message size sum allowed on the producer queue.
