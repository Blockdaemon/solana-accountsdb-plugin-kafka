# Solana AccountsDB Plugin for Kafka

Kafka publisher for use with Solana's [plugin framework](https://docs.solana.com/developing/plugins/geyser-plugins).

## Quick Start

**Want to see data flowing immediately?** Use this minimal config:

```json
{
  "libpath": "target/release/libsolana_accountsdb_plugin_kafka.so",
  "kafka": {
    "bootstrap.servers": "localhost:9092"
  },
  "filters": [{
    "update_account_topic": "solana.testnet.account_updates",
    "slot_status_topic": "solana.testnet.slot_status",
    "transaction_topic": "solana.testnet.transactions",
    "publish_all_accounts": true
  }]
}
```

This will publish **all** account updates, transactions, and slot status to Kafka. Perfect for testing and development.

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

**⚠️ WARNING: This example config will NOT publish most data by default!**

The following config is a minimal example that demonstrates the structure, but with `publish_all_accounts: false` and no `program_filters`, you'll only see slot status updates. For testing, consider using the scenarios below.

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

**For Testing/Development (Recommended):**
```json
{
  "libpath": "target/release/libsolana_accountsdb_plugin_kafka.so",
  "kafka": {
    "bootstrap.servers": "localhost:9092"
  },
  "filters": [{
    "update_account_topic": "solana.testnet.account_updates",
    "slot_status_topic": "solana.testnet.slot_status",
    "transaction_topic": "solana.testnet.transactions",
    "publish_all_accounts": true
  }]
}
```

### Reference

- `libpath`: Path to Kafka plugin
- `kafka`: [`librdkafka` config options](https://github.com/edenhill/librdkafka/blob/master/CONFIGURATION.md). Common options include:
  - `statistics.interval.ms`: Enables Prometheus metrics collection (set to 1000ms or higher)
  - `queue.buffering.max.messages`: Controls producer buffer size
  - `queue.buffering.max.kbytes`: Controls producer buffer size in KB
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

**⚠️ IMPORTANT: Understanding how filtering works is crucial for getting data flowing to Kafka!**

The plugin uses a **whitelist approach** for filtering. By default, most events are filtered out unless you explicitly configure what you want to see.

#### How Filtering Works

1. **Account Updates**: Only published if the account's owner program is in `program_filters` OR the account address is in `account_filters`
2. **Transactions**: Only published if they involve accounts from programs in `program_filters` OR specific accounts in `account_filters`
3. **Slot Status**: Always published (not affected by filters)
4. **Program Ignores**: Blacklist of programs/accounts to exclude (applied after whitelist filtering)

#### Common Filtering Scenarios

**Scenario 1: See Everything (Recommended for testing)**
```json
{
  "publish_all_accounts": true,
  "program_filters": [],
  "account_filters": []
}
```

**Scenario 2: See Specific Programs Only**
```json
{
  "publish_all_accounts": false,
  "program_filters": [
    "11111111111111111111111111111111",  // System Program
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"  // Token Program
  ]
}
```

**Scenario 3: See Specific Accounts Only**
```json
{
  "publish_all_accounts": false,
  "account_filters": [
    "YourAccountAddressHere111111111111111111111111"
  ]
}
```

#### Troubleshooting: No Data in Kafka?

If you're not seeing messages in Kafka despite successful slot processing:

1. **Check your filters**: Make sure you have either `publish_all_accounts: true` or specific `program_filters`/`account_filters`
2. **Verify topics**: Ensure your topic names are correct and Kafka is running
3. **Check program ignores**: Make sure you're not accidentally filtering out everything with overly restrictive `program_ignores`
4. **Test with minimal config**: Start with `publish_all_accounts: true` to verify the plugin is working

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

## Troubleshooting

### Common Issues

#### 1. No Data in Kafka Topics

**Symptoms**: Solana validator shows slot processing but no messages appear in Kafka topics.

**Causes & Solutions**:
- **Filtering too restrictive**: Set `publish_all_accounts: true` or add specific `program_filters`
- **Wrong topic names**: Verify your topic names match exactly
- **Kafka connection issues**: Check if Kafka is running and accessible
- **Plugin not loaded**: Verify the plugin path in `libpath` is correct

**Quick Test**: Use the Quick Start config above to verify the plugin works.

#### 2. Only Slot Status Messages Appear

**Cause**: This is expected behavior with the default example config! Slot status is always published, but account updates and transactions require explicit filter configuration.

**Solution**: Add `publish_all_accounts: true` or configure `program_filters`.

#### 3. Plugin Fails to Load

**Common Causes**:
- **Version mismatch**: Ensure Solana and plugin are built with identical Rust/Solana versions
- **Library path**: Check `libpath` points to the correct `.so` or `.dylib` file
- **Permissions**: Ensure the plugin file is readable by the Solana process

#### 4. High Memory Usage

**Cause**: Large Kafka producer buffers can consume significant memory.

**Solution**: Adjust buffer settings:
```json
{
  "kafka": {
    "queue.buffering.max.messages": "10000",
    "queue.buffering.max.kbytes": "1048576"
  }
}
```

### Debugging Tips

1. **Start Simple**: Begin with `publish_all_accounts: true` to verify basic functionality
2. **Check Topics**: Use Kafdrop or `kafka-console-consumer` to verify topics exist
3. **Monitor Metrics**: Enable Prometheus metrics to see message counts and errors
4. **Verify Filters**: Double-check your filter configuration matches your expectations

### Getting Help

If you're still having issues:
1. Check this troubleshooting section
2. Review the filtering documentation above
3. Try the Quick Start configuration
4. Open an issue with your config and error details
