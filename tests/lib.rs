extern crate idx;
use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};

use idx::*;
use idx::util::*;

lazy_static! {
    pub static ref CACHE: Arc<Mutex<Cache>> = Arc::from(Mutex::from(Cache::from_path("test_cache").unwrap()));
}

#[test]
fn test_load_cache() {
    let _ = CACHE.lock();
}

#[test]
fn test_file_amounts() {
    let mut cache = CACHE.lock().unwrap();
    let index = cache.index(19).unwrap();

    assert_eq!(15432, index.get_total_files());
}

#[test]
fn test_retrieve_filedata() {
    let mut provider = FileProvider::from(&*CACHE);
    provider.index(19);

    let whip_id = 4152;

    provider.archive(&(whip_id >> 8));
    let data = provider.request(&(whip_id & 0xff));

    assert_eq!(vec![97, 16, 55, 98, 3, 31, 0], data.deconstruct());
}

#[test]
fn test_hashnames() {
    let mut provider = FileProvider::from(&*CACHE);
    provider.index(8);
    provider.archive(&String::from("logo"));

    let data = provider.request(&0);

    assert_ne!(0, data.deconstruct().len())
}

#[test]
fn test_defprovider() {
    struct Bogus {
        op: u8
    }

    impl DefParser for Bogus {
        fn parse_buff(mut buffer: databuffer::DataBuffer) -> Self {
            Self {
                op: if buffer.len() == 0 {
                    0
                } else {
                    buffer.read_u8()
                }
            }
        }
    }

    let mut provider = DefProvider::<Bogus>::with(&*CACHE, 8);

    let data = provider.get_def(&1, &0, 1);

    assert_ne!(data.op, 0);

    let data = provider.get_def(&String::from("logo"), &0, 1);

    assert_ne!(data.op, 0);
}