# Solana AccountsDB Plugin for Kafka

Kafka publisher for use with Solana's [plugin framework](https://docs.solana.com/developing/plugins/geyser-plugins).

## Installation

### Binary releases

Find binary releases [here](https://github.com/Blockdaemon/solana-accountsdb-plugin-kafka/releases).

### Building from source

#### Prerequisites

You will need version 3.15 or later of the protobuf compiler `protoc` installed, since it is required for the `--experimental_allow_proto3_optional` option.

Note that as of this writing, Ubuntu 22.04 still has an obsolete of `protoc`.

Previously, you could use `protocbuf-compiiler` from Debian trixie, but trixie's libc6 is no longer compatible with Ubuntu 22.04.

You will need to use Ubuntu 24.04 or later, or Debian 12 or later.

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
    "partitioner": "murmur2_random",
    "statistics.interval.ms": "1000"
  },
  "shutdown_timeout_ms": 30000,
  "filters": [{
    "update_account_topic": "solana.testnet.account_updates",
    "slot_status_topic": "solana.testnet.slot_status",
    "transaction_topic": "solana.testnet.transactions",
    "program_ignores": [
      "Sysvar1111111111111111111111111111111111111",
      "Vote111111111111111111111111111111111111111"
    ],
    "publish_all_accounts": false,
    "wrap_messages": false
  }]
}
```

### Reference

- `libpath`: Path to Kafka plugin
- `kafka`: [`librdkafka` config options](https://github.com/edenhill/librdkafka/blob/master/CONFIGURATION.md).
- `shutdown_timeout_ms`: Time the plugin is given to flush out all messages to Kafka upon exit request.
- `prometheus`: Optional port to provide metrics in Prometheus format.
- `filters`: Vec of filters with next fields:
  - `update_account_topic`: Topic name of account updates. Omit to disable.
  - `slot_status_topic`: Topic name of slot status update. Omit to disable.
  - `transaction_topic`: Topic name of transaction update. Omit to disable.
  - `program_ignores`: Account addresses to ignore (see Filtering below).
  - `program_filters`: Solana program IDs to include.
  - `account_filters`: Solana accounts to include.
  - `publish_all_accounts`: Publish all accounts on startup. Omit to disable.
  - `include_vote_transactions`: Include Vote transactions.
  - `include_failed_transactions`: Include failed transactions.
  - `wrap_messages`: Wrap all messages in a unified wrapper object. Omit to disable (see Message Wrapping below).

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
Therefore it is crucial to choose a sufficiently large buffer.

The buffer size can be controlled using `librdkafka` config options, including:

- `queue.buffering.max.messages`: Maximum number of messages allowed on the producer queue.
- `queue.buffering.max.kbytes`: Maximum total message size sum allowed on the producer queue.

## Enhanced Analytics Support

The plugin now provides significantly richer analytics data for blockchain monitoring and analysis, enhancing all supported Solana transaction formats.

### Transaction Types

The plugin supports multiple Solana transaction formats:

- **Legacy Transactions**: Traditional Solana message format
- **V0 Transactions**: Versioned transactions with address lookup tables (LUTs)

All transaction types are enhanced with comprehensive analytics metadata.

### Enhanced Analytics Features

All transactions provide additional analytics data that can be used for:

#### Performance Monitoring
- **Compute Units**: Actual compute units consumed by transactions
- **Pricing Information**: Compute unit pricing from ComputeBudget instructions
- **Cost Analysis**: Transaction fees and compute costs

#### Error Analysis & Debugging
- **Error Detection**: Reliable error status from transaction metadata
- **Success Tracking**: Transaction success/failure status
- **Error Details**: Structured error information without log parsing

#### Address Intelligence
- **Address Lookup Tables**: Support for V0 LUT transactions
- **Loaded Address Details**: Index and writable status for loaded addresses
- **Account Metadata**: Enhanced account information and versioning

#### Slot & Network Analytics
- **Confirmation Status**: Smart confirmation counting based on slot status
- **Status Descriptions**: Human-readable slot status descriptions
- **Progress Tracking**: Slot progression monitoring

### Message Schema Enhancements

The protobuf schema has been enhanced with analytics fields:

#### UpdateAccountEvent
- `data_version`: Account data version for change tracking
- `is_startup`: Whether this is a startup account update
- `account_age`: Approximate account age in slots

#### SlotStatusEvent
- `is_confirmed`: Whether slot is confirmed by supermajority
- `confirmation_count`: Confirmation level (0-2 based on status)
- `status_description`: Human-readable status description

#### TransactionEvent
- `compute_units_consumed`: Actual compute units used
- `compute_units_price`: Compute unit pricing in micro-lamports
- `total_cost`: Total transaction cost (fee + compute)
- `instruction_count`: Number of instructions in transaction
- `account_count`: Number of accounts involved
- `execution_time_ns`: Execution time in nanoseconds
- `is_successful`: Transaction success status
- `execution_logs`: Detailed execution logs
- `error_details`: Detailed error information
- `confirmation_count`: Number of confirmations

#### LoadedAddresses
- `writable_info`: Detailed writable address information
- `readonly_info`: Detailed readonly address information

### Configuration for Analytics Features

Analytics features are enabled by default and require no additional configuration. The plugin automatically:

- Detects transaction format (Legacy or V0)
- Extracts available metadata fields
- Provides enhanced analytics where data is available
- Maintains backwards compatibility with existing consumers

### Use Cases

Analytics enhancements enable new use cases:

- **Performance Monitoring**: Track compute unit usage and costs
- **Error Analysis**: Monitor transaction failures and success rates
- **Network Analytics**: Analyze slot confirmation patterns
- **Address Intelligence**: Monitor address lookup table usage
- **Cost Optimization**: Analyze transaction pricing and efficiency
