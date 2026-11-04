use bytes::{Bytes, BytesMut};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{net::UdpSocket, time::Instant};
use tracing::{debug, error, info, trace};

use crate::structs::udp::{
    AppState,
    Event::{self, *},
    InfoHash, Peer, PeerId, UdpAction, UdpResponse, REANNOUNCE_INTERVAL,
};

pub async fn start_utp_serve(host_on: SocketAddr, app_state: Arc<AppState>) -> anyhow::Result<()> {
    let udp_socket = UdpSocket::bind(host_on).await.map_err(|e| {
        let error = format!("Error Binding to UDP Socket: {:?}", e);
        error!(error);
        anyhow::anyhow!(error)
    })?;

    info!("[UDP]: Listening on: {}", host_on);

    loop {
        let mut buf_mut = BytesMut::with_capacity(1024);

        let (buf_size, remote_socket) = udp_socket.recv_buf_from(&mut buf_mut).await?;

        let response = handle_request(
            buf_mut.clone().freeze(),
            buf_size,
            remote_socket,
            &app_state,
        )
        .await?;

        buf_mut.clear();
        buf_mut.extend_from_slice(&response.to_bytes());
        udp_socket.send_to(&buf_mut, remote_socket).await?;
    }
}

pub async fn handle_request(
    buffer: Bytes,
    buf_size: usize,
    remote_socket: SocketAddr,
    app_state: &Arc<AppState>,
) -> anyhow::Result<UdpResponse> {
    let transaction_id = u32::from_be_bytes(buffer[12..16].try_into()?);

    let Ok(action) = UdpAction::try_from(u32::from_be_bytes(buffer[8..12].try_into()?)) else {
        let response = UdpResponse::Error {
            transaction_id,
            message: "Message does not contain an action".to_string(),
        };
        return Ok(response);
    };

    let transaction_id = u32::from_be_bytes(buffer[12..16].try_into()?);

    match action {
        UdpAction::Connect => {
            if buf_size < 16 {
                let response = UdpResponse::Error {
                    transaction_id,
                    message: "Connect action needs request to be least 16 bytes".to_string(),
                };
                return Ok(response);
            }

            let connection_id = call_gen_connection_id(app_state);

            app_state.connections.insert(
                connection_id,
                (remote_socket, {
                    Instant::now() + Duration::from_secs(REANNOUNCE_INTERVAL)
                }),
            );

            let response = UdpResponse::Connect {
                transaction_id,
                connection_id,
            };

            trace!("Sent response {:?}", response);

            Ok(response)
        }
        UdpAction::Announce => {
            if buf_size < 20 {
                let response = UdpResponse::Error {
                    transaction_id,
                    message: "Announce action requires request to be at least 20 bytes".to_string(),
                };
                return Ok(response);
            }

            let Ok(info_hash): Result<InfoHash, _> = buffer[16..36].try_into() else {
                let response = UdpResponse::Error {
                    transaction_id,
                    message: "InfoHash field needs to be present in request".to_string(),
                };
                return Ok(response);
            };

            let Ok(peer_id): Result<PeerId, _> = buffer[36..56].try_into() else {
                let response = UdpResponse::Error {
                    transaction_id,
                    message: "You don't have a peer_id".to_string(),
                };
                return Ok(response);
            };

            let downloaded = u64::from_be_bytes(buffer[56..64].try_into()?);
            let left = u64::from_be_bytes(buffer[64..72].try_into()?);
            let uploaded = u64::from_be_bytes(buffer[72..80].try_into()?);

            let Ok(event) = Event::try_from(u32::from_be_bytes(buffer[80..84].try_into()?)) else {
                let response = UdpResponse::Error {
                    transaction_id,
                    message: "Event Field is missing in request".to_string(),
                };
                return Ok(response);
            };

            let ip_bytes = u32::from_be_bytes(buffer[84..88].try_into()?);
            let ip = if ip_bytes == 0 {
                remote_socket.ip()
            } else {
                IpAddr::V4(Ipv4Addr::from_bits(ip_bytes))
            };

            let port = u16::from_be_bytes(buffer[96..98].try_into()?);
            let numwant = u32::from_be_bytes(buffer[92..96].try_into()?);

            let peer = Peer {
                peer_id,
                ip,
                port,
                uploaded,
                downloaded,
                left,
                last_announce: Instant::now(),
            };

            let info_hash_stuff = app_state.info_hashes.entry(info_hash).or_default();

            let mut peer_list: Vec<Peer> = Vec::new();

            match event {
                None => {
                    debug!("Recieved None event, sending peer list");

                    info_hash_stuff.peers.insert(peer_id, peer);

                    let peer_list_to_send: Vec<Peer> = info_hash_stuff
                        .peers
                        .iter()
                        .filter(|entry| {
                            let list_peer_id = entry.peer_id;
                            list_peer_id != peer_id
                        })
                        .map(|entry| entry.value().clone())
                        .take(numwant as usize)
                        .collect();

                    peer_list = peer_list_to_send;
                }
                Started => {
                    debug!("Recieved Started event, sending peer list and adding +1 to leaches");
                    info_hash_stuff.peers.insert(peer_id, peer);

                    {
                        let stats = info_hash_stuff.stats.clone();
                        stats.leachers.fetch_add(1, Ordering::Relaxed);
                    }

                    let peer_list_to_send: Vec<Peer> = info_hash_stuff
                        .peers
                        .iter()
                        .filter(|entry| {
                            let list_peer_id = entry.peer_id;
                            list_peer_id != peer_id
                        })
                        .map(|entry| entry.value().clone())
                        .take(numwant as usize)
                        .collect();

                    peer_list = peer_list_to_send;
                }
                Completed => {
                    debug!("Recieved Completed event, sending peer list and adding +1 to seeders and -1 to leaches");

                    info_hash_stuff.peers.insert(peer_id, peer);

                    {
                        let stats = info_hash_stuff.stats.clone();
                        stats.leachers.fetch_sub(1, Ordering::Relaxed);
                        stats.seeders.fetch_add(1, Ordering::Relaxed);
                    }

                    let peer_list_to_send: Vec<Peer> = info_hash_stuff
                        .peers
                        .iter()
                        .filter(|entry| {
                            let list_peer_id = entry.peer_id;
                            list_peer_id != peer_id
                        })
                        .map(|entry| entry.value().clone())
                        .take(numwant as usize)
                        .collect();

                    peer_list = peer_list_to_send;
                }
                Stopped => {
                    debug!("Recieved Stopped event, depends on what we are doing");

                    info_hash_stuff.peers.remove(&peer_id);

                    // Peer who is disconnecting wasn't seeding
                    if peer.left > 0 {
                        info_hash_stuff
                            .stats
                            .leachers
                            .fetch_sub(1, Ordering::Relaxed);
                        debug!("[Stopped]: removing leacher");
                    } else {
                        info_hash_stuff
                            .stats
                            .seeders
                            .fetch_sub(1, Ordering::Relaxed);
                        debug!("[Stopped]: removing seeder");
                    }
                }
            }

            let stats = info_hash_stuff.stats.clone();
            let leechers = stats.leachers.load(Ordering::Relaxed);
            let seeders = stats.seeders.load(Ordering::Relaxed);

            let response = UdpResponse::Announce {
                transaction_id,
                interval: {
                    let dur = Duration::from_secs(REANNOUNCE_INTERVAL);
                    let interval_secs: u32 = dur.as_secs().try_into().unwrap_or(u32::MAX);
                    interval_secs.to_be_bytes()
                },
                leechers,
                seeders,
                peers: peer_list,
            };

            trace!("Sent response {:?}", response);

            Ok(response)
        }
        _ => {
            let response = UdpResponse::Error {
                transaction_id,
                message: "Scraping is not implimented yet* on this tracker".to_string(),
            };

            Ok(response)
        }
    }
}

pub fn call_gen_connection_id(app_state: &Arc<AppState>) -> u64 {
    loop {
        match try_gen_connection_id(app_state) {
            Ok(id) => {
                break id;
            }
            Err(_) => continue,
        }
    }
}

fn try_gen_connection_id(app_state: &Arc<AppState>) -> anyhow::Result<u64> {
    let id = rand::random::<u64>();
    match app_state.connections.contains_key(&id) {
        false => Ok(id),
        true => Err(anyhow::anyhow!("need to regen id")),
    }
}
