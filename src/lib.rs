// Copyright 2022 Blockdaemon Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin;

mod config;
mod event;
mod filter;
mod plugin;
mod prom;
mod publisher;
mod version;

pub use {
    config::{Config, Producer},
    event::*,
    filter::Filter,
    plugin::KafkaPlugin,
    prom::PrometheusService,
    publisher::Publisher,
};

#[no_mangle]
#[allow(improper_ctypes_definitions)]
/// # Safety
///
/// This function returns a pointer to the Kafka Plugin box implementing trait GeyserPlugin.
///
/// The Solana validator and this plugin must be compiled with the same Rust compiler version and Solana core version.
/// Loading this plugin with mismatching versions is undefined behavior and will likely cause memory corruption.
pub unsafe extern "C" fn _create_plugin() -> *mut dyn GeyserPlugin {
    let plugin = KafkaPlugin::new();
    let plugin: Box<dyn GeyserPlugin> = Box::new(plugin);
    Box::into_raw(plugin)
}
