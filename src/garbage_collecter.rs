use rayon::prelude::*;
use std::{sync::Arc, time::Duration};
use tokio::time::{sleep, Instant};
use tracing::{debug, trace};

use crate::structs::udp::{AppState, ConnectionId};

pub async fn cnnection_gargabe_collector_worker(app_state: Arc<AppState>) -> anyhow::Result<()> {
    loop {
        debug!("Before: {:?}", app_state.connections);

        let now = Instant::now();

        let to_remove: Vec<ConnectionId> = app_state
            .connections
            .par_iter()
            .filter_map(|entry| {
                let (connection_id, (_addr, time_to_check)) = entry.pair();

                if now.checked_duration_since(*time_to_check).is_some() {
                    Some(*connection_id)
                } else {
                    None
                }
            })
            .collect();

        to_remove.into_par_iter().for_each(|key| {
            app_state.connections.remove(&key);
        });

        trace!("Removed stale connections");
        debug!("After: {:?}", app_state.connections);

        sleep(Duration::from_secs(190)).await
    }
}

pub async fn info_hash_gargabe_collector_worker(app_state: Arc<AppState>) -> anyhow::Result<()> {
    loop {
        debug!("Before: {:?}", app_state.info_hashes);
        app_state.info_hashes.par_iter_mut().for_each(|torrents| {
            torrents.peers.retain(|key, _torrent| !key.is_empty());
        });

        trace!("Cleared out empty torrents.");
        debug!("After: {:?}", app_state.info_hashes);

        sleep(Duration::from_mins(5)).await
    }
}
