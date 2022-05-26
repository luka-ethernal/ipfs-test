use std::time::SystemTime;
use std::{str::FromStr, time::Duration};

use anyhow::Result;
use ipfs_embed::Key;
use ipfs_embed::{DefaultParams, Ipfs, NetworkConfig, StorageConfig, ToLibp2p};
use ipfs_test::util::keypair_from_seed;
use tokio::join;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let keypair = keypair_from_seed(2);

    let sweep_interval = Duration::from_secs(60);
    let now = SystemTime::duration_since(&SystemTime::now(), SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let path_buf = std::path::PathBuf::from_str(format!("consumer_{:?}", now).as_str()).unwrap();
    let storage = StorageConfig::new(None, 10, sweep_interval);
    let network = NetworkConfig::new(path_buf, keypair);
    let ipfs = Ipfs::<DefaultParams>::new(ipfs_embed::Config { storage, network }).await?;

    let mut stream = ipfs.listen_on("/ip4/127.0.0.1/tcp/0".parse()?)?;
    let events = tokio::spawn(async move {
        while let Some(e) = stream.next().await {
            log::info!("Event: {:?}", e);
        }
    });

    let mut swarm_stream = ipfs.swarm_events();
    let swarm_events = tokio::spawn(async move {
        while let Some(e) = swarm_stream.next().await {
            log::info!("Swarm Event: {:?}", e);
        }
    });

    let producer = keypair_from_seed(1).to_peer_id();

    ipfs.bootstrap(&[(producer, "/ip4/127.0.0.1/tcp/2001".parse().unwrap())])
        .await
        .unwrap();

    let consume = tokio::spawn(async move {
        let mut num = 1u32;
        loop {
            let key = Key::from(format!("ref_num:{}", num).as_bytes().to_vec());

            if let Ok(rec) = ipfs.get_record(&key, ipfs_embed::Quorum::One).await {
                let block_cid = String::from_utf8_lossy(rec[0].record.value.as_slice())
                    .parse()
                    .unwrap();
                let peers = ipfs.peers();
                log::info!("peer num={}", peers.len());

                match ipfs.fetch(&block_cid, peers).await {
                    Ok(data) => {
                        log::info!("Got data: {:?}", String::from_utf8_lossy(data.data()));
                        num += 1;
                    }
                    Err(e) => {
                        log::warn!("No data, error: {:?}", e);
                    }
                }
            } else {
                log::warn!("No record, waiting...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });

    let _ = join!(swarm_events, events, consume);

    Ok(())
}
