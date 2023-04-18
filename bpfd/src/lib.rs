// SPDX-License-Identifier: (MIT OR Apache-2.0)
// Copyright Authors of bpfd

mod bpf;
mod certs;
mod command;
mod errors;
mod multiprog;
#[path = "oci-utils/mod.rs"]
mod oci_utils;
mod rpc;
mod static_program;
mod utils;

use std::{fs::remove_file, net::SocketAddr, path::Path};

use anyhow::Context;
use bpf::BpfManager;
use bpfd_api::{config::Config, util::directories::RTDIR_FS_MAPS, v1::loader_server::LoaderServer};
pub use certs::get_tls_config;
use command::{AttachType, Command, NetworkMultiAttach, TcProgram, TracepointProgram};
use errors::BpfdError;
use log::{info, warn};
use rpc::{intercept, BpfdLoader};
use static_program::get_static_programs;
use tokio::{net::UnixListener, sync::mpsc};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Server, ServerTlsConfig};
use utils::{get_ifindex, set_map_permissions};

use crate::command::{
    Metadata, NetworkMultiAttachInfo, Program, ProgramData, ProgramType, XdpProgram,
};

pub async fn serve(config: Config, static_program_path: &str) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel(32);
    let endpoint = &config.grpc.endpoint;

    // Listen on Unix socket
    let unix = endpoint.unix.clone();
    if Path::new(&unix).exists() {
        // Attempt to remove the socket, since bind fails if it exists
        remove_file(&unix)?;
    }

    let uds = UnixListener::bind(&unix)?;
    let uds_stream = UnixListenerStream::new(uds);

    let loader = BpfdLoader::new(tx.clone());

    let serve = Server::builder()
        .add_service(LoaderServer::new(loader))
        .serve_with_incoming(uds_stream);

    tokio::spawn(async move {
        info!("Listening on {}", unix);
        if let Err(e) = serve.await {
            eprintln!("Error = {e:?}");
        }
    });

    // Listen on TCP socket
    let addr = SocketAddr::new(
        endpoint
            .address
            .parse()
            .unwrap_or_else(|_| panic!("failed to parse listening address '{}'", endpoint.address)),
        endpoint.port,
    );

    let loader = BpfdLoader::new(tx);

    let (ca_cert, identity) = get_tls_config(&config.tls)
        .await
        .context("CA Cert File does not exist")?;

    let tls_config = ServerTlsConfig::new()
        .identity(identity)
        .client_ca_root(ca_cert);

    let serve = Server::builder()
        .tls_config(tls_config)?
        .add_service(LoaderServer::with_interceptor(loader, intercept))
        .serve(addr);

    tokio::spawn(async move {
        info!("Listening on {addr}");
        if let Err(e) = serve.await {
            eprintln!("Error = {e:?}");
        }
    });

    let mut bpf_manager = BpfManager::new(&config);
    bpf_manager.rebuild_state()?;

    let static_programs = get_static_programs(static_program_path).await?;

    // Load any static programs first
    if !static_programs.is_empty() {
        for prog in static_programs {
            let uuid = bpf_manager.add_program(prog)?;
            info!("Loaded static program with UUID {}", uuid)
        }
    };

    // Start receiving messages
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Load {
                location,
                section_name,
                global_data,
                program_type,
                attach_type:
                    AttachType::NetworkMultiAttach(NetworkMultiAttach {
                        iface,
                        priority,
                        proceed_on,
                        direction,
                        position: _,
                    }),
                username,
                responder,
            } => {
                let res = if let Ok(if_index) = get_ifindex(&iface) {
                    // If proceedOn is empty, then replace with the default
                    let proc_on = if proceed_on.0.is_empty() {
                        match program_type {
                            command::ProgramType::Xdp => command::ProceedOn::default_xdp(),
                            command::ProgramType::Tc => command::ProceedOn::default_tc(),
                            _ => proceed_on,
                        }
                    } else {
                        // FIXME: when proceed-on is supported for TC programs just return: proceed_on
                        match program_type {
                            command::ProgramType::Xdp => proceed_on,
                            command::ProgramType::Tc => {
                                warn!("proceed-on config not supported yet for TC and may have unintended behavior");
                                proceed_on
                            }
                            _ => proceed_on,
                        }
                    };

                    let prog_data_result =
                        ProgramData::new(location, section_name.clone(), global_data, username)
                            .await;

                    match prog_data_result {
                        Ok(prog_data) => {
                            let prog_result: Result<Program, BpfdError> = match program_type {
                                command::ProgramType::Xdp => Ok(Program::Xdp(XdpProgram {
                                    data: prog_data.clone(),
                                    info: NetworkMultiAttachInfo {
                                        if_index,
                                        current_position: None,
                                        metadata: command::Metadata {
                                            priority,
                                            // This could have been overridden by image tags
                                            name: prog_data.section_name,
                                            attached: false,
                                        },
                                        proceed_on: proc_on,
                                        if_name: iface,
                                    },
                                })),
                                command::ProgramType::Tc => Ok(Program::Tc(TcProgram {
                                    data: prog_data.clone(),
                                    info: NetworkMultiAttachInfo {
                                        if_index,
                                        current_position: None,
                                        metadata: command::Metadata {
                                            priority,
                                            name: prog_data.section_name,
                                            attached: false,
                                        },
                                        proceed_on: proc_on,
                                        if_name: iface,
                                    },
                                    direction: direction.unwrap(),
                                })),
                                _ => Err(BpfdError::InvalidProgramType(program_type.to_string())),
                            };

                            match prog_result {
                                Ok(prog) => bpf_manager.add_program(prog),
                                Err(e) => Err(e),
                            }
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(BpfdError::InvalidInterface)
                };

                // If program was successfully loaded, allow map access by bpfd group members.
                if let Ok(uuid) = res {
                    let maps_dir = format!("{RTDIR_FS_MAPS}/{uuid}");
                    set_map_permissions(&maps_dir).await;
                }

                // Ignore errors as they'll be propagated to caller in the RPC status
                let _ = responder.send(res);
            }
            Command::Load {
                location,
                section_name,
                global_data,
                attach_type: AttachType::SingleAttach(attach),
                username,
                responder,
                program_type,
            } => {
                let prog_data_result =
                    ProgramData::new(location, section_name, global_data, username).await;

                let res = match prog_data_result {
                    Ok(prog_data) => {
                        let prog_result: Result<Program, BpfdError> = match program_type {
                            command::ProgramType::Tracepoint => {
                                Ok(Program::Tracepoint(TracepointProgram {
                                    data: prog_data,
                                    info: attach,
                                }))
                            }
                            _ => Err(BpfdError::InvalidProgramType(program_type.to_string())),
                        };

                        match prog_result {
                            Ok(prog) => bpf_manager.add_program(prog),
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                };

                // If program was successfully loaded, allow map access by bpfd group members.
                if let Ok(uuid) = res {
                    let maps_dir = format!("{RTDIR_FS_MAPS}/{uuid}");
                    set_map_permissions(&maps_dir).await;
                }

                // Ignore errors as they'll be propagated to caller in the RPC status
                let _ = responder.send(res);
            }
            Command::Unload {
                id,
                username,
                responder,
            } => {
                let res = bpf_manager.remove_program(id, username);
                // Ignore errors as they'll be propagated to caller in the RPC status
                let _ = responder.send(res);
            }
            Command::List { responder } => {
                let progs = bpf_manager.list_programs();
                // Ignore errors as they'll be propagated to caller in the RPC status
                let _ = responder.send(progs);
            }
        }
    }
    Ok(())
}
