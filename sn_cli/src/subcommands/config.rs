// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::operations::config::{Config, NetworkInfo};
use clap::Subcommand;
use color_eyre::Result;
use std::path::PathBuf;
use tracing::debug;
use url::Url;

#[derive(Subcommand, Debug)]
pub enum ConfigSubCommands {
    #[clap(name = "add", subcommand)]
    /// Add a config setting
    Add(SettingAddCmd),
    #[clap(name = "remove", subcommand)]
    /// Remove a config setting
    Remove(SettingRemoveCmd),
    #[clap(name = "clear")]
    /// Remove all config settings and network maps
    Clear,
}

#[derive(Subcommand, Debug)]
pub enum SettingAddCmd {
    #[clap(name = "network")]
    Network {
        /// Network name
        network_name: String,
        /// Local path or a remote URL to fetch the network map from
        contacts_file_location: String,
    },
    // #[clap(name = "contact")]
    // Contact {
    //    /// Contact friendly name
    //    name: String,
    //    /// SafeId of the contact
    //    safeid: String,
    // },
}

#[derive(Subcommand, Debug)]
pub enum SettingRemoveCmd {
    #[clap(name = "network")]
    Network {
        /// Network to remove
        network_name: String,
    },
    // #[clap(name = "contact")]
    // Contact {
    //    /// Name of the contact to remove
    //    name: String,
    // },
}

pub async fn config_commander(cmd: Option<ConfigSubCommands>, config: &mut Config) -> Result<()> {
    match cmd {
        Some(ConfigSubCommands::Add(SettingAddCmd::Network {
            network_name,
            contacts_file_location,
        })) => {
            if Url::parse(contacts_file_location.as_str()).is_ok() {
                config
                    .add_network(
                        &network_name,
                        NetworkInfo::Remote(contacts_file_location, None),
                    )
                    .await?;
            } else {
                let path = PathBuf::from(contacts_file_location);
                config
                    .add_network(&network_name, NetworkInfo::Local(path, None))
                    .await?;
            }
        }
        // Some(ConfigSubCommands::Add(SettingAddCmd::Contact { name, safeid })) => {}
        Some(ConfigSubCommands::Remove(SettingRemoveCmd::Network { network_name })) => {
            config.remove_network(&network_name).await?
        }
        // Some(ConfigSubCommands::Remove(SettingRemoveCmd::Contact { name })) => {}
        Some(ConfigSubCommands::Clear) => {
            config.clear().await?;
            debug!("Config settings cleared out");
        }
        None => config.print_networks().await,
    }

    Ok(())
}
