use std::{
    net::{IpAddr, SocketAddr},
    sync::atomic::{AtomicU32, Ordering},
};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

pub type InfoHash = [u8; 20];
pub type PeerId = [u8; 20];

pub type ConnectionId = u64;
pub type TransactionId = u32;

pub type Connections = DashMap<ConnectionId, (SocketAddr, Instant)>;
pub type InfoHashes = DashMap<InfoHash, TorrentInfo>;

#[derive(Debug, Clone, Default)]
pub struct TorrentInfo {
    pub peers: DashMap<PeerId, Peer>,
    pub stats: TorrentStats,
}

#[derive(Debug, Default)]
pub struct TorrentStats {
    pub seeders: AtomicU32,
    pub leachers: AtomicU32,
}

impl Clone for TorrentStats {
    fn clone(&self) -> Self {
        Self {
            seeders: AtomicU32::new(self.seeders.load(Ordering::Relaxed)),
            leachers: AtomicU32::new(self.leachers.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub connections: Connections,
    pub info_hashes: InfoHashes,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Peer {
    pub peer_id: PeerId,
    pub ip: IpAddr,
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub last_announce: Instant,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum UdpRequest {
    Connect {
        transaction_id: u32,
    },
    Announce {
        connection_id: ConnectionId,
        transaction_id: TransactionId,
        info_hash: InfoHash,
        peer_id: PeerId,
        downloaded: u64,
        left: u64,
        uploaded: u64,
        event: Event,
        ip: IpAddr,
        key: u32,
        num_want: i32,
        port: u16,
    },
    Scrape {
        connection_id: ConnectionId,
        transaction_id: TransactionId,
        info_hashes: Vec<InfoHash>,
    },
}

#[derive(Debug)]
pub enum UdpResponse {
    Connect {
        transaction_id: TransactionId,
        connection_id: ConnectionId,
    },
    Announce {
        transaction_id: TransactionId,
        interval: [u8; 4],
        leechers: u32,
        seeders: u32,
        peers: Vec<Peer>,
    },
    Error {
        transaction_id: TransactionId,
        message: String,
    },
}

impl UdpResponse {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            UdpResponse::Connect {
                transaction_id,
                connection_id,
            } => {
                let mut buf = Vec::with_capacity(16);

                buf.extend_from_slice(&0u32.to_be_bytes());
                buf.extend_from_slice(&transaction_id.to_be_bytes());
                buf.extend_from_slice(&connection_id.to_be_bytes());

                buf
            }
            UdpResponse::Announce {
                transaction_id,
                interval,
                leechers,
                seeders,
                peers,
            } => {
                let mut buf = Vec::with_capacity(20 + peers.len() * 6);

                buf.extend_from_slice(&1u32.to_be_bytes());
                buf.extend_from_slice(&transaction_id.to_be_bytes());
                buf.extend_from_slice(interval);
                buf.extend_from_slice(&leechers.to_be_bytes());
                buf.extend_from_slice(&seeders.to_be_bytes());

                for peer in peers {
                    match peer.ip {
                        IpAddr::V4(ipv4) => {
                            buf.extend_from_slice(&ipv4.octets());
                            buf.extend_from_slice(&peer.port.to_be_bytes());
                        }
                        IpAddr::V6(_) => {
                            // handle later <BEP 7>
                        }
                    }
                }

                buf
            }
            UdpResponse::Error {
                transaction_id,
                message,
            } => {
                let mut buf = Vec::with_capacity(8);

                buf.extend_from_slice(&3u32.to_be_bytes());
                buf.extend_from_slice(&transaction_id.to_be_bytes());
                buf.extend_from_slice(message.as_bytes());

                buf
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum UdpAction {
    Connect = 0,
    Announce = 1,
    Scrape = 2,
}

impl TryFrom<u32> for UdpAction {
    type Error = ();
    fn try_from(v: u32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(UdpAction::Connect),
            1 => Ok(UdpAction::Announce),
            2 => Ok(UdpAction::Scrape),
            _ => Err(()),
        }
    }
}

impl From<UdpAction> for u32 {
    fn from(a: UdpAction) -> u32 {
        a as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    None,
    Completed,
    Started,
    Stopped,
}

impl TryFrom<u32> for Event {
    type Error = ();
    fn try_from(v: u32) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Event::None),
            1 => Ok(Event::Completed),
            2 => Ok(Event::Started),
            3 => Ok(Event::Stopped),
            _ => Err(()),
        }
    }
}

impl From<Event> for u32 {
    fn from(e: Event) -> u32 {
        match e {
            Event::None => 0,
            Event::Completed => 1,
            Event::Started => 2,
            Event::Stopped => 3,
        }
    }
}
