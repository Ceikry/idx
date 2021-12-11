extern crate idx;
use std::sync::{Arc, Mutex};

use idx::*;
use idx::util::*;

#[test]
fn test_load_cache() {
    if let Some(mut _cache) = Cache::from_path("test_cache"){
        let cache = Arc::from(Mutex::from(_cache));
        let mut file_provider = FileProvider::from(cache);

        let whip_id = 0 as u32;
        let whip_archive = file_provider.index(19).archive(&(whip_id >> 8));

        let whip_data = whip_archive.request(&(whip_id & 0xff));
        println!("{:?}", whip_data.to_bytes());

        let whip_id = 15219 as u32;
        let whip_archive = file_provider.index(19).archive(&(whip_id >> 8));

        let whip_data = whip_archive.request(&(whip_id & 0xff));
        println!("{:?}", whip_data.to_bytes());

        let whip_id = 13412 as u32;
        let whip_archive = file_provider.index(19).archive(&(whip_id >> 8));

        let whip_data = whip_archive.request(&(whip_id & 0xff));
        println!("{:?}", whip_data.to_bytes());

        let whip_id = 9316 as u32;
        let whip_archive = file_provider.index(19).archive(&(whip_id >> 8));

        let whip_data = whip_archive.request(&(whip_id & 0xff));
        println!("{:?}", whip_data.to_bytes())
    }
}

#[test]
fn test_file_amounts() {
    let mut cache = Cache::from_path("test_cache").unwrap();
    let index = cache.index(19).unwrap();

    println!("{}", index.get_total_files());
}

