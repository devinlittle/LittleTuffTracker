use rayon::prelude::*;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::time::{sleep, Instant};
use tracing::{debug, trace};

use crate::structs::udp::{AppState, ConnectionId, InfoHash, REANNOUNCE_INTERVAL};

pub async fn cnnection_gargabe_collector_worker(app_state: Arc<AppState>) -> anyhow::Result<()> {
    loop {
        debug!(
            "[Garbage Collector]: Connection Before: {:?}",
            app_state.connections
        );

        let now = Instant::now();

        let to_remove: Vec<ConnectionId> = app_state
            .connections
            .par_iter()
            .filter_map(|entry| {
                let (connection_id, (_addr, time_to_check)) = entry.pair();

                if now >= *time_to_check {
                    Some(*connection_id)
                } else {
                    None
                }
            })
            .collect();

        if !to_remove.is_empty() {
            trace!(
                "[Garbage Collector]: Removed {} stale connections",
                to_remove.len()
            );
        };

        to_remove.into_par_iter().for_each(|key| {
            app_state.connections.remove(&key);
        });

        debug!(
            "[Garbage Collector]: Connection After: {:?}",
            app_state.connections
        );

        let sweep_frequency = std::cmp::min(REANNOUNCE_INTERVAL / 2, 30);
        sleep(Duration::from_secs(sweep_frequency.min(10))).await
    }
}

pub async fn info_hash_gargabe_collector_worker(app_state: Arc<AppState>) -> anyhow::Result<()> {
    loop {
        let timeout = Duration::from_secs(REANNOUNCE_INTERVAL * 2);
        let now = Instant::now();

        debug!("Before: {:?}", app_state.info_hashes);
        app_state
            .info_hashes
            .par_iter_mut()
            .for_each(|mut torrents| {
                let torrents = torrents.value_mut();

                torrents.peers.retain(|_, peer| {
                    let is_active = now.duration_since(peer.last_announce) < timeout;

                    if !is_active {
                        if peer.left == 0 {
                            torrents.stats.seeders.fetch_sub(1, Ordering::Relaxed);
                        } else {
                            torrents.stats.leachers.fetch_sub(1, Ordering::Relaxed);
                        }
                    }

                    is_active
                });
            });

        let empty_hashes: Vec<InfoHash> = app_state
            .info_hashes
            .iter()
            .filter_map(|entry| {
                if entry.value().peers.is_empty() {
                    Some(*entry.key())
                } else {
                    None
                }
            })
            .collect();

        let removed_count = empty_hashes.len();
        for hash in empty_hashes {
            app_state.info_hashes.remove(&hash);
        }
        if removed_count > 0 {
            trace!(
                "[Garbage Collector]: Removed {} dead torrents with 0 peers",
                removed_count
            );
        }

        debug!(
            "After cleanup: {} torrents tracked",
            app_state.info_hashes.len()
        );

        sleep(Duration::from_secs(300)).await
    }
}
