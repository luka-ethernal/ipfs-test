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

// struct Data {
//     data: Vec<u8>
// }

// impl Data {
// 	/// Cell refrence in format `block_number:column_number:row_number`
// 	pub fn reference(&self, num: u32) -> String { format!("ref_num:{}", num) }

// 	pub fn hash(&self, num: u32) -> Multihash { Code::Sha3_256.digest(self.reference(num).as_bytes()) }

// 	pub fn cid(&self, num: u32) -> Cid { Cid::new_v1(IpldCodec::Raw.into(), self.hash(num)) }

// 	// Block data isn't encoded
// 	// TODO: optimization - multiple cells inside a single block (per appID)
// 	pub fn to_ipfs_block(self, num: u32) -> Block<DefaultParams> {
// 		Block::<DefaultParams>::new_unchecked(self.cid(num), self.data)
// 	}
// }

fn get_block_cid(num: u32) -> Cid {
    let s = format!("ref_num:{}", num);
    let h = Code::Sha3_256.digest(s.as_bytes());
    Cid::new_v1(IpldCodec::Raw.into(), h)
}

#[tokio::main]
async fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Debug).unwrap();
    let keypair = keypair_from_seed(2);

    let sweep_interval = Duration::from_secs(60);
    let path_buf = std::path::PathBuf::from_str("consumer").unwrap();
    let storage = StorageConfig::new(None, 10, sweep_interval);
    let mut network = NetworkConfig::new(path_buf, keypair);
    // network.streams = None;
    let ipfs = Ipfs::<DefaultParams>::new(ipfs_embed::Config { storage, network }).await?;

    let mut stream = ipfs.listen_on("/ip4/127.0.0.1/tcp/2002".parse()?)?;
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
                ipfs.sync(&block_cid, peers.clone());

                match ipfs.fetch(&block_cid, peers).await {
                    Ok(data) => {
                        log::info!("Got data: {:?}", data.data());
                        num += 1;
                    }
                    Err(e) => {
                        log::warn!("No data, error: {:?}", e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            } else {
                log::warn!("No record, error");
            }
        }
    });

    join!(swarm_events, events, consume);

    Ok(())
}
