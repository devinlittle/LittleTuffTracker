use bytes::{Bytes, BytesMut};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{net::UdpSocket, time::Instant};
use tracing::{debug, error, info, trace};

use crate::structs::udp::{
    AppState, Event, InfoHash, Peer, PeerId, UdpAction, UdpResponse, REANNOUNCE_INTERVAL,
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

        let response = match handle_request(
            buf_mut.clone().freeze(),
            buf_size,
            remote_socket,
            &app_state,
        )
        .await
        {
            Ok(response) => response,
            Err(_) => UdpResponse::Error {
                transaction_id: 0,
                message: "Fatal Error when communicating to tracker".to_string(),
            },
        };

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
    let Some(transaction_bytes) = buffer.get(12..16) else {
         let response = UdpResponse::Error {
            transaction_id: 0,
            message: "Message does not contain a transaction id".to_string(),
        };
        return Ok(response);
    };

    let transaction_id = u32::from_be_bytes(transaction_bytes.try_into()?);

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

            let connection_id = u64::from_be_bytes(buffer[0..8].try_into()?);

            if let Some(mut entry) = app_state.connections.get_mut(&connection_id) {
                entry.value_mut().1 = Instant::now() + Duration::from_secs(REANNOUNCE_INTERVAL * 2);
            }

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
            let stats = &info_hash_stuff.stats;

            let mut peer_list: Vec<Peer> = Vec::new();

            if matches!(event, Event::Stopped) {
                debug!("Recieved Stopped event, removing peer");

                if let Some((_peer_id, old_peer)) = info_hash_stuff.peers.remove(&peer_id) {
                    if old_peer.left == 0 {
                        stats.seeders.fetch_sub(1, Ordering::Relaxed);
                        debug!("[Stopped]: removed seeder");
                    } else {
                        stats.leachers.fetch_sub(1, Ordering::Relaxed);
                        debug!("[Stopped]: removed leecher");
                    }
                }
            } else {
                let old_peer = info_hash_stuff.peers.insert(peer_id, peer.clone());

                let was_seeder = old_peer.as_ref().is_some_and(|p| p.left == 0);
                let was_leecher = old_peer.as_ref().is_some_and(|p| p.left > 0);
                let is_seeder = peer.left == 0;
                let is_leecher = peer.left > 0;

                if old_peer.is_none() {
                    // brand new peer appearing

                    if is_seeder {
                        stats.seeders.fetch_add(1, Ordering::Relaxed);
                        debug!("[Announce]: Registered new seeder");
                    } else {
                        stats.leachers.fetch_add(1, Ordering::Relaxed);
                        debug!("[Announce]: Registered new leecher");
                    }
                } else {
                    // an existing peer updating stats
                    if was_leecher && is_seeder {
                        stats.leachers.fetch_sub(1, Ordering::Relaxed);
                        stats.seeders.fetch_add(1, Ordering::Relaxed);
                        debug!("[Announce]: Peer completed download, transitioned to seeder");
                    } else if was_seeder && is_leecher {
                        stats.seeders.fetch_sub(1, Ordering::Relaxed);
                        stats.leachers.fetch_add(1, Ordering::Relaxed);
                        debug!("[Announce]: Peer became leecher");
                    }
                }

                peer_list = info_hash_stuff
                    .peers
                    .iter()
                    .filter(|entry| *entry.key() != peer_id)
                    .map(|entry| entry.value().clone())
                    .take(numwant as usize)
                    .collect();
            }

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
