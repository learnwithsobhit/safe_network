// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::operations::{
    config::{Config, NetworkInfo, NetworkLauncher},
    node::*,
};
use clap::Subcommand;
use color_eyre::{eyre::eyre, Result};
use std::{net::SocketAddr, path::PathBuf};

use sn_api::DEFAULT_NETWORK_CONTACTS_FILE_NAME;

const NODES_DATA_DIR_NAME: &str = "baby-fleming-nodes";
const LOCAL_NODE_DIR_NAME: &str = "local-node";

#[derive(Subcommand, Debug)]
pub enum NodeSubCommands {
    /// Gets the version of `sn_node` binary
    BinVersion {
        #[clap(long = "node-path", env = "SN_NODE_PATH")]
        node_path: Option<PathBuf>,
    },
    #[clap(name = "install")]
    /// Install an sn_node binary
    Install {
        /// Optional destination directory path for the installation. The SN_NODE_PATH environment
        /// variable can also be used to supply this path. If this argument is not used, the
        /// binary will be installed at ~/.safe/node/sn_node, or the equivalent user directory
        /// path on Windows.
        #[clap(long = "node-path", env = "SN_NODE_PATH")]
        node_path: Option<PathBuf>,
        /// Specify the version of sn_node to install. If not supplied, the latest version will be
        /// installed. Note: just the version number should be supplied, with no 'v' prefix.
        #[clap(short = 'v', long)]
        version: Option<String>,
    },
    #[clap(name = "join")]
    /// Join an existing network
    Join {
        /// The name of a network from the `networks` command list. Use this argument to join one
        /// of those networks.
        #[clap(long = "network-name")]
        network_name: String,
        /// Path of the directory where sn_node is located (default is ~/.safe/node/). The SN_NODE_PATH env var can also be used to set the path
        #[clap(long = "node-dir-path", env = "SN_NODE_PATH")]
        node_dir_path: Option<PathBuf>,
        /// Verbosity level for nodes logs
        #[clap(short = 'y', parse(from_occurrences))]
        verbosity: u8,
        /// Local address to be used for the node.
        ///
        /// When unspecified, the node will listen on `0.0.0.0` with a random unused port. If you're
        /// running a local-only network, you should set this to `127.0.0.1:0` to prevent any external
        /// traffic from reaching the node (but note that the node will also be unable to connect to
        /// non-local nodes).
        ///
        /// This option can also be used when you're trying to join a remote network, but your join
        /// request was rejected because the other nodes were unable to reach your node. In this
        /// case, you can setup 'manual' port forwarding on your router, then use this option to set
        /// the address where local packets are being forwarded to. For example, --local-addr
        /// 192.168.1.50:12000.
        #[clap(short = 'a', long)]
        local_addr: Option<SocketAddr>,
        /// External address of the node, to use when writing connection info.
        ///
        /// If unspecified, it will be queried from a peer; if there are no peers, the `local-addr` will
        /// be used, if specified.
        ///
        /// This option can also be used when you're trying to join a remote network, but your join
        /// request was rejected because the other nodes were unable to reach your node. In this
        /// case, you can setup 'manual' port forwarding on your router, then use this option to set
        /// the public IP address of your router. For example, --public-addr 79.71.42.39:12000.
        #[clap(short = 'p', long)]
        public_addr: Option<SocketAddr>,
        /// Delete all data from a previous node running on the same PC
        #[clap(long = "clear-data")]
        clear_data: bool,
        /// Set this flag if you're connecting to a network where all the nodes are running
        /// locally. This will launch the node and skip any port forwarding.
        #[clap(short = 'l', long)]
        local: bool,
        /// Use this flag to skip the automated, software-based port forwarding on the node binary.
        ///
        /// This option can also be used when you're trying to join a remote network, but your join
        /// request was rejected because the other nodes were unable to reach your node. In this
        /// case, you can setup 'manual' port forwarding on your router, then use this option to set
        /// disable the software-based port forwarding in the node binary.
        #[clap(long)]
        skip_auto_port_forwarding: bool,
    },
    #[clap(name = "run-baby-fleming")]
    /// Run nodes to form a local single-section Safe network
    Run {
        /// Path of the directory where sn_node is located (default is ~/.safe/node/). The SN_NODE_PATH env var can also be used to set the path
        #[clap(long = "node-dir-path", env = "SN_NODE_PATH")]
        node_dir_path: Option<PathBuf>,
        /// Interval in seconds between launching each of the nodes
        #[clap(short = 'i', long, default_value = "1")]
        interval: u64,
        /// Number of nodes to be launched
        #[clap(long = "nodes", default_value = "11")]
        num_of_nodes: u8,
        /// IP to be used to launch the local nodes.
        #[clap(long = "ip")]
        ip: Option<String>,
    },
    /// Shutdown all running nodes processes
    #[clap(name = "killall")]
    Killall {
        /// Path of the sn_node executable used to launch the processes with (default ~/.safe/node/sn_node). The SN_NODE_PATH env var can be also used to set this path
        #[clap(long = "node-path", env = "SN_NODE_PATH")]
        node_path: Option<PathBuf>,
    },
    #[clap(name = "update")]
    /// Update to latest sn_node released version
    Update {
        #[clap(long = "node-path")]
        /// Path of the sn_node executable to update (default ~/.safe/node/). The SN_NODE_PATH env var can be also used to set the path
        #[clap(long = "node-path", env = "SN_NODE_PATH")]
        node_path: Option<PathBuf>,
    },
}

pub async fn node_commander(
    cmd: Option<NodeSubCommands>,
    config: &mut Config,
    network_launcher: &mut Box<impl NetworkLauncher>,
) -> Result<()> {
    match cmd {
        Some(NodeSubCommands::BinVersion { node_path }) => node_version(node_path),
        Some(NodeSubCommands::Install { node_path, version }) => {
            let target_dir_path = if let Some(path) = node_path {
                path
            } else {
                let mut path = config.network_contacts_dir.clone();
                path.pop();
                path.push("node");
                path
            };
            // We run this command in a separate thread to overcome a conflict with
            // the self_update crate as it seems to be creating its own runtime.
            let handler = std::thread::spawn(|| node_install(target_dir_path, version));
            handler
                .join()
                .map_err(|err| eyre!("Failed to run self update: {:?}", err))?
        }
        Some(NodeSubCommands::Join {
            network_name,
            node_dir_path,
            verbosity,
            local_addr,
            public_addr,
            clear_data,
            local,
            skip_auto_port_forwarding: disable_port_forwarding,
        }) => {
            config.switch_to_network(network_name.as_str()).await?;
            let node_directory_path = if let Some(path) = node_dir_path {
                path
            } else {
                let mut path = config.network_contacts_dir.clone();
                path.pop();
                path.push("node");
                path
            };

            let default_network_contacts_path = config
                .network_contacts_dir
                .join(DEFAULT_NETWORK_CONTACTS_FILE_NAME);

            node_join(
                network_launcher,
                node_directory_path,
                LOCAL_NODE_DIR_NAME,
                verbosity,
                local_addr,
                public_addr,
                clear_data,
                local,
                disable_port_forwarding,
                default_network_contacts_path,
            )
        }
        Some(NodeSubCommands::Run {
            node_dir_path,
            interval,
            num_of_nodes,
            ip,
        }) => {
            let node_directory_path = if let Some(path) = node_dir_path {
                path
            } else {
                let mut path = config.network_contacts_dir.clone();
                path.pop();
                path.push("node");
                path
            };
            node_run(
                network_launcher,
                node_directory_path,
                NODES_DATA_DIR_NAME,
                interval,
                &num_of_nodes.to_string(),
                ip,
            )?;

            // add the network using default network contacts file
            let (network_contacts, _) = config.read_default_network_contacts().await?;
            config.write_network_contacts(&network_contacts).await?;

            let actual_path = config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key()));
            config
                .add_network("baby-fleming", NetworkInfo::Local(actual_path, None))
                .await?;

            Ok(())
        }
        Some(NodeSubCommands::Killall { node_path }) => node_shutdown(node_path),
        Some(NodeSubCommands::Update { node_path }) => node_update(node_path),
        None => Err(eyre!("Missing node subcommand")),
    }
}

#[cfg(test)]
mod test {
    use crate::operations::config::{Config, NetworkLauncher};
    use color_eyre::Report;
    use futures::executor::block_on;

    pub struct FakeNetworkLauncher {
        pub launch_args: Vec<String>,
        pub config: Config,
    }

    impl NetworkLauncher for FakeNetworkLauncher {
        fn launch(&mut self, args: Vec<String>, _interval: u64) -> Result<(), Report> {
            self.launch_args.extend(args);
            block_on(async {
                let _ = self
                    .config
                    .store_dummy_network_contacts_and_set_default(1)
                    .await?;
                Ok(())
            })
        }

        fn join(&mut self, args: Vec<String>) -> Result<(), Report> {
            self.launch_args.extend(args);
            Ok(())
        }
    }
}

#[cfg(test)]
mod run_command {
    use super::test::FakeNetworkLauncher;
    use super::{node_commander, NodeSubCommands, NODES_DATA_DIR_NAME};
    use crate::operations::config::Config;
    use crate::operations::node::SN_NODE_EXECUTABLE;
    use assert_fs::prelude::*;
    use color_eyre::eyre::eyre;
    use color_eyre::Result;
    use std::path::PathBuf;

    #[tokio::test]
    async fn should_use_optionally_supplied_node_directory_path() -> Result<()> {
        let custom_node_dir = assert_fs::TempDir::new()?;
        let tmp_dir = assert_fs::TempDir::new()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: Some(PathBuf::from(custom_node_dir.path())),
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());

        assert!(launcher.launch_args.iter().any(|x| x == "--node-path"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == custom_node_dir.path().join(SN_NODE_EXECUTABLE)));

        Ok(())
    }

    #[tokio::test]
    async fn should_use_default_node_directory_path() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--node-path"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == node_dir.path().join(SN_NODE_EXECUTABLE)));

        Ok(())
    }

    #[tokio::test]
    async fn should_use_default_node_data_directory_path() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--nodes-dir"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == node_dir.path().join(NODES_DATA_DIR_NAME)));

        Ok(())
    }

    #[tokio::test]
    async fn should_use_custom_node_data_directory_path() -> Result<()> {
        let custom_node_dir = assert_fs::TempDir::new()?;
        let tmp_dir = assert_fs::TempDir::new()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: Some(PathBuf::from(custom_node_dir.path())),
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--nodes-dir"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == custom_node_dir.path().join(NODES_DATA_DIR_NAME)));

        Ok(())
    }

    #[tokio::test]
    async fn should_create_the_node_data_directory_if_it_does_not_exist() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let node_data_dir = node_dir.child(NODES_DATA_DIR_NAME);
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        node_data_dir.assert(predicates::path::is_dir());

        Ok(())
    }

    #[tokio::test]
    async fn should_use_optionally_supplied_interval_value() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 10,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--interval"));
        assert!(launcher.launch_args.iter().any(|x| x == "10000"));

        Ok(())
    }

    #[tokio::test]
    async fn should_use_optionally_supplied_num_of_nodes_value() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 15,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--num-nodes"));
        assert!(launcher.launch_args.iter().any(|x| x == "15"));

        Ok(())
    }

    #[tokio::test]
    async fn should_use_optionally_supplied_ip_address() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 11,
            ip: Some("10.10.0.1".to_string()),
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--ip"));
        assert!(launcher.launch_args.iter().any(|x| x == "10.10.0.1"));

        Ok(())
    }

    #[tokio::test]
    async fn should_use_local_flag_if_no_ip_is_supplied() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--local"));

        Ok(())
    }

    #[tokio::test]
    async fn should_add_baby_fleming_to_networks_list() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Run {
            node_dir_path: None,
            interval: 1,
            num_of_nodes: 11,
            ip: None,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert_eq!(config.networks_iter().count(), 1);

        let (network_name, _) = config
            .networks_iter()
            .next()
            .ok_or_else(|| eyre!("failed to read network from config"))?;
        assert_eq!(network_name, "baby-fleming");
        Ok(())
    }
}
//
#[cfg(test)]
mod join_command {
    use super::test::FakeNetworkLauncher;
    use super::{node_commander, NodeSubCommands, LOCAL_NODE_DIR_NAME};
    use crate::operations::config::{Config, NetworkInfo};
    use crate::operations::node::SN_NODE_EXECUTABLE;
    use assert_fs::prelude::*;
    use color_eyre::Result;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;

    #[tokio::test]
    async fn should_connect_to_network_using_network_name_argument() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        let (default_network_contacts, _) = config.read_default_network_contacts().await?;
        assert_eq!(network_contacts, default_network_contacts);
        Ok(())
    }

    #[tokio::test]
    async fn should_use_optionally_supplied_node_directory_path_argument() -> Result<()> {
        let custom_node_dir = assert_fs::TempDir::new()?;
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: Some(PathBuf::from(custom_node_dir.path())),
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--node-path"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == custom_node_dir.path().join(SN_NODE_EXECUTABLE)));
        Ok(())
    }

    #[tokio::test]
    async fn should_use_default_node_directory_path() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--nodes-dir"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == node_dir.path().join(LOCAL_NODE_DIR_NAME)));
        Ok(())
    }

    #[tokio::test]
    async fn should_use_custom_node_data_directory_path() -> Result<()> {
        let custom_node_dir = assert_fs::TempDir::new()?;
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: Some(PathBuf::from(custom_node_dir.path())),
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--nodes-dir"));
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| PathBuf::from(x) == custom_node_dir.path().join(LOCAL_NODE_DIR_NAME)));
        Ok(())
    }

    #[tokio::test]
    async fn should_pass_the_skip_auto_port_forwarding_flag() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: true,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher
            .launch_args
            .iter()
            .any(|x| x == "--skip-auto-port-forwarding"));
        Ok(())
    }

    #[tokio::test]
    async fn should_use_custom_local_addr_argument() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0)),
            public_addr: None,
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--local-addr"));
        assert!(launcher.launch_args.iter().any(|x| x == "127.0.0.1:0"));
        Ok(())
    }

    #[tokio::test]
    async fn should_use_custom_public_addr_argument() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: None,
            public_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(10, 10, 10, 10)),
                5000,
            )),
            clear_data: false,
            local: false,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--public-addr"));
        assert!(launcher.launch_args.iter().any(|x| x == "10.10.10.10:5000"));
        Ok(())
    }

    #[tokio::test]
    async fn should_use_clear_data_argument() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: true,
            local: true,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "--clear-data"));
        Ok(())
    }

    #[tokio::test]
    async fn should_use_verbosity_argument() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 3,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: true,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        assert!(launcher.launch_args.iter().any(|x| x == "-yyy"));
        Ok(())
    }

    #[tokio::test]
    async fn should_create_the_node_data_directory_if_it_does_not_exist() -> Result<()> {
        let tmp_dir = assert_fs::TempDir::new()?;
        let node_dir = tmp_dir.child(".safe/node");
        node_dir.create_dir_all()?;
        let node_data_dir = node_dir.child(LOCAL_NODE_DIR_NAME);
        let mut config = Config::create_config(&tmp_dir, None).await?;

        let network_contacts = config
            .store_dummy_network_contacts_and_set_default(1)
            .await?
            .pop()
            .unwrap();
        let baby_fleming = NetworkInfo::Local(
            config
                .network_contacts_dir
                .join(format!("{:?}", network_contacts.genesis_key())),
            None,
        );
        config.add_network("baby-fleming", baby_fleming).await?;

        let mut launcher = Box::new(FakeNetworkLauncher {
            launch_args: Vec::new(),
            config: config.clone(),
        });

        let cmd = NodeSubCommands::Join {
            network_name: String::from("baby-fleming"),
            node_dir_path: None,
            verbosity: 0,
            local_addr: None,
            public_addr: None,
            clear_data: false,
            local: true,
            skip_auto_port_forwarding: false,
        };

        let result = node_commander(Some(cmd), &mut config, &mut launcher).await;

        assert!(result.is_ok());
        node_data_dir.assert(predicates::path::is_dir());

        Ok(())
    }
}
