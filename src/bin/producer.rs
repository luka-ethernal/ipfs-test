use std::{path::Path, str::FromStr, time::Duration};

use anyhow::Result;
use ipfs_embed::multiaddr::multihash::MultihashDigest;
use ipfs_embed::{
    multiaddr::multihash::{Code, Multihash},
    Block, Cid, Config, DefaultParams, Ipfs, NetworkConfig, StorageConfig, ToLibp2p,
};
use ipfs_embed::{Key, Record};
use ipfs_test::util::keypair_from_seed;
use libipld::IpldCodec;
use tokio::join;
use tokio_stream::StreamExt;

struct Data {
    data: Vec<u8>,
}

impl Data {
    pub fn hash(&self) -> Multihash {
        Code::Sha3_256.digest(self.data.as_slice())
    }

    pub fn cid(&self) -> Cid {
        Cid::new_v1(IpldCodec::Raw.into(), self.hash())
    }

    // Block data isn't encoded
    // TODO: optimization - multiple cells inside a single block (per appID)
    pub fn to_ipfs_block(self, num: u32) -> Block<DefaultParams> {
        Block::<DefaultParams>::new(self.cid(), self.data).unwrap()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let keypair = keypair_from_seed(1);

    let sweep_interval = Duration::from_secs(60);
    let path_buf = std::path::PathBuf::from_str("producer").unwrap();
    let storage = StorageConfig::new(None, 10, sweep_interval);
    let mut network = NetworkConfig::new(path_buf, keypair);
    // network.streams = None;
    let ipfs = Ipfs::<DefaultParams>::new(ipfs_embed::Config { storage, network }).await?;

    let mut stream = ipfs.listen_on("/ip4/127.0.0.1/tcp/2001".parse()?)?;
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
    
    let consumer = keypair_from_seed(2).to_peer_id();

    ipfs.bootstrap(&[(consumer, "/ip4/127.0.0.1/tcp/2002".parse().unwrap())])
        .await
        .unwrap();

    let produce = tokio::spawn(async move {
        for num in 1..10000u32 {
            let raw = format!("example data {}", num).as_bytes().to_vec();
            let data = Data { data: raw.clone() };
            let cid = data.cid(num);
            log::info!("inserting {} block, cid={}", num, cid);
            let pin = ipfs.create_temp_pin().unwrap();
            ipfs.temp_pin(&pin, &cid).unwrap();
            ipfs.insert(&data.to_ipfs_block(num)).unwrap();
            ipfs.flush().await.unwrap();
            let peers = ipfs.peers();
            ipfs.sync(&cid, peers);
            while let Err(e) = ipfs.put_record(
                Record::new(
                    format!("ref_num:{}", num).as_bytes().to_vec(),
                    cid.to_string().as_bytes().to_vec(),
                ),
                ipfs_embed::Quorum::One,
            )
            .await {
                log::warn!("No quorum, retrying again... error: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }

            tokio::time::sleep(Duration::from_secs(5)).await;

            let peers = ipfs.peers();
            let data2 = ipfs.fetch(&cid, peers).await.unwrap();
            assert_eq!(raw.as_slice(), data2.data(), "Data mismatch");
            log::info!("verified {} block", num);
        }
    });

    join!(swarm_events, events, produce);

    Ok(())
}
