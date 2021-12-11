extern crate idx;
use std::sync::{Arc, Mutex};

use idx::*;
use idx::util::*;

#[test]
fn test_load_cache() {
    if let Some(mut _cache) = Cache::from_path("test_cache"){
        let cache = Arc::from(Mutex::from(_cache));
        let mut file_provider = FileProvider::from(cache);

        for id in 0..15432 {
            let whip_archive = file_provider.index(19).archive(&(id >> 8));
    
            whip_archive.request(&(id & 0xff));
        }
    }
}

#[test]
fn test_file_amounts() {
    let mut cache = Cache::from_path("test_cache").unwrap();
    let index = cache.index(19).unwrap();

    println!("{}", index.get_total_files());
}

