use std::{io::{Seek, SeekFrom, Read, BufReader}, fs::{File, OpenOptions}, path::PathBuf, collections::HashMap, sync::{Arc, Mutex, MutexGuard}, hash::Hasher};
use databuffer::DataBuffer;
use crate::util::decompress_container_data;

pub mod util;

type IdxFileOpt<'a> = Option<&'a mut CacheIndex>;

/**
  The Cache struct is the top-level representation of the cache itself,
  all data within the cache is accessed via this struct.
  It is highly recommended (and in fact necessary for DefProvider) 
  that the cache is wrapped in a Arc'd Mutex, like so:
  ```ignore
  let cache = Arc::from(Mutex::from(Cache::from_path("test_cache")));
  ```

  Once the Cache is creating using its [`Cache::from_path("/path/to/cache")`] method,
  all archives and file containers will be populated, though
  none of the data will be read for individual files.

  For a recommended method of retrieving raw file data from the cache, see [`util::FileProvider`].
  
  For tips on implementing a full-blown Definition Provider, see [`util::DefProvider`].
 */
pub struct Cache {
    pub data_file: Arc<Mutex<BufReader<File>>>,
    pub indices: HashMap<u8, CacheIndex>
}

impl Cache {
    pub fn from_path(path: &str) -> Option<Self> {
        let mut path_buff = PathBuf::new();
        path_buff.push(path);
        path_buff.push("main_file_cache.idx255");

        let mut info_file = match OpenOptions::new()
        .read(true)
        .open(&path_buff) {
            Ok(n) => n,
            Err(e) => {
                println!("Failed opening info/reference file: {:?}, Error: {}", &path_buff, e);
                return None;
            }
        };

        path_buff.clear();
        path_buff.push(path);
        path_buff.push("main_file_cache.dat2");

        let data_file = match OpenOptions::new()
        .read(true)
        .open(&path_buff) {
            Ok(n) => Arc::from(Mutex::from(BufReader::new(n))),
            Err(e) => {
                println!("Failed opening data file: {:?}, Error: {}", &path_buff, e);
                return None;
            }
        };

        let num_files = info_file.metadata().unwrap().len() / 6;
        println!("{}", num_files);
        let _ = info_file.seek(SeekFrom::Start(0));

        let mut info = CacheIndex::from(255, 500000, BufReader::new(info_file), IdxContainerInfo::new());
        let mut indices = HashMap::<u8, CacheIndex>::new();

        for i in 0..num_files {
            path_buff.clear();
            path_buff.push(path);
            path_buff.push(format!("main_file_cache.idx{}",&i));

            let file = match OpenOptions::new().read(true).open(&path_buff) {
                Ok(n) => BufReader::new(n),
                Err(e) => {
                    println!("Error reading idx {}: {}", i, e);
                    continue;
                }
            };

            let container_data = match CacheIndex::container_data(&mut info, data_file.lock().unwrap(), i as u32) {
                Some(n) => n,
                None => {
                    println!("Unable to get container data.");
                    Vec::new()
                }
            };

            let container_info = IdxContainerInfo::from(container_data);

            let index = CacheIndex::from(i as u8, 1000000, file, container_info);
            indices.insert(i as u8, index);
        }

        indices.insert(255, info);

        Some(Self {
            data_file,
            indices
        })
    }

    pub fn index(&mut self, idx: usize) -> IdxFileOpt {
        return match self.indices.get_mut(&(idx as u8)) {
            Some(n) => Some(n),
            None => {
                println!("No such index exists: {}", idx);
                None
            }
        }
    }
}

pub struct CacheIndex {
    file_id: u8,
    file: BufReader<File>,
    max_container_size: u32,
    pub container_info: IdxContainerInfo,
    last_archive_id: u32
}

impl CacheIndex {
    fn from(file_id: u8, max_size: u32, file: BufReader<File>, container_info: IdxContainerInfo) -> Self {
        Self {
            file_id,
            max_container_size: max_size,
            file,
            container_info,
            last_archive_id: 0
        }
    }

    fn get_container_by_name_hash(&mut self, hash: u32) -> u32 {
        match self.container_info.containers.iter().filter(|(_,c)| c.name_hash == hash).last() {
            Some((c,_)) => *c,
            None => hash
        }
    }

    pub fn container_data(&mut self, mut data_file: MutexGuard<BufReader<File>>, archive_id: u32) -> Option<Vec<u8>> {
        let mut file_buff: [u8; 520] = [0; 520];
        let mut data: [u8;6] = [0; 6];

        if archive_id <= 1 || self.last_archive_id == 0 {
            let _ = self.file.seek(SeekFrom::Start(6 * archive_id as u64));
        } else if self.last_archive_id != archive_id - 1 {
            let seek_offset = 6 * (archive_id as i64 - (self.last_archive_id as i64 + 1));
            let _ = self.file.seek_relative(seek_offset);
        }

        self.last_archive_id = archive_id;

        let _ = match self.file.read(&mut data) {
            Ok(_) => {}
            Err(e) => {
                println!("Error reading from info file: {}", e);
            }
        };

        let container_size = (data[2] as u32) + (((data[0] as u32) << 16) + (((data[1] as u32) << 8) & 0xff00));
        let mut sector = ((data[3] as i32) << 16) - (-((0xff & data[4] as i32) << 8) - (data[5] as i32 & 0xff)); 

        if container_size > self.max_container_size {
            println!("Container Size greater than Max Container Size! {} > {}", container_size, self.max_container_size);
            None
        } else if sector <= 0 {
            println!("Sector <= 0! {}", sector);
            None
        } else {
            let mut container_data = Vec::<u8>::new();

            let mut data_read_count = 0;
            let mut part: u32 = 0;

            let initial_dfile_pos = data_file.seek(SeekFrom::Start(520 * (sector as u64))).unwrap() as i64;

            while container_size > data_read_count {
                if sector == 0 {
                    println!("Sector == 0!");
                    return None;
                }

                let seek_target: i64 = 520 * (sector as i64);
                let current_pos = initial_dfile_pos + (data_read_count as i64) + (part as i64 * 8);

                if current_pos != seek_target {
                    let _ = data_file.seek_relative(520);
                }

                let mut data_to_read = container_size - data_read_count;

                if data_to_read > 512 {
                    data_to_read = 512;
                }

                let bytes_read = data_file.read(&mut file_buff).unwrap();

                if data_to_read + 8 > bytes_read as u32 {
                    let _ = data_file.seek(SeekFrom::Start(520 * (sector as u64)));

                    let _ = data_file.read(&mut file_buff);
                }

                let current_container_id = (0xff & file_buff[1] as u32) + ((0xff & file_buff[0] as u32) << 8);
                let current_part = ((0xff & file_buff[2] as u32) << 8) + (0xff & file_buff[3] as u32);
                let next_sector = (0xff & file_buff[6] as u32) + ((0xff & file_buff[5] as u32) << 8) + ((0xff & file_buff[4] as u32) << 16);
                let current_idx_file_id = 0xff & file_buff[7] as u32;

                if archive_id != (current_container_id as u32) || current_part != part || self.file_id != (current_idx_file_id as u8) {
                    println!("Multipart failure! {} != {} || {} != {} || {} != {}", archive_id, current_container_id, current_part, part, self.file_id, current_idx_file_id);
                    return None;
                }

                let upper_bound = 8 + data_to_read as usize;

                container_data.extend_from_slice(&file_buff[8..upper_bound]);
                data_read_count += data_to_read;

                part += 1;
                sector = next_sector as i32;
            }

            Some(container_data)
        }
    }

    pub fn get_total_files(&mut self) -> u32 {
        self.container_info.container_indices.sort_unstable();

        let last_archive_id = *self.container_info.container_indices.last().unwrap();
        let last_archive = self.container_info.containers.get(&last_archive_id).unwrap();

        let last_archive_file_amount = last_archive.file_indices.len();
        let other_file_amounts = (self.container_info.container_indices.len() - 1) * 256;
        
        (last_archive_file_amount + other_file_amounts) as u32
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct IdxContainerInfo {
    pub protocol: u8,
    pub revision: u32,
    pub crc: u32,
    container_indices: Vec<u32>,
    pub containers: HashMap<u32, IdxContainer>,
    named_files: bool,
    whirlpool: bool
}

impl IdxContainerInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from(packed_data: Vec<u8>) -> Self {
        let mut crc_hasher = crc32fast::Hasher::new();
        crc_hasher.update(&packed_data);
        let crc = crc_hasher.finalize();


        let mut data = match decompress_container_data(packed_data) {
            Some(n) => DataBuffer::with_vec(n),
            None => {
                println!("Unable to decompress container data.");
                return Self::new();
            }
        };

        let protocol = data.read_u8();
        
        if protocol != 5 && protocol != 6 {
            println!("Invalid protocol while parsing container info: {}", protocol);
            Self::new()
        } else {
            let revision = match protocol {
                5 => 0,
                _ => data.read_u32()
            };

            let settings_hash = data.read_u8();
            let files_named = (0x1 & settings_hash) != 0;
            let whirlpool = (0x2 & settings_hash) != 0;

            let mut containers = HashMap::<u32, IdxContainer>::new();
            let mut container_indices = Vec::<u32>::new();
            let num_indices = data.read_u16();

            for i in 0..num_indices {
                container_indices.push((data.read_u16() as u32) + match i {
                    0 => 0,
                    _ => *container_indices.last().unwrap()
                });

                containers.insert(*container_indices.last().unwrap(), IdxContainer::new());
            }

            if files_named {
                for c in container_indices.iter().take(num_indices as usize) {
                    containers.get_mut(c).unwrap().name_hash = data.read_u32();
                }
            }

            let mut file_hashes: HashMap<u32, [u8;64]> = HashMap::new();

            if whirlpool {
                for c in container_indices.iter().take(num_indices as usize) {
                    let mut buf: [u8; 64] = [0; 64];
                    let _ = data.read(&mut buf);
                    file_hashes.insert(*c, buf);
                }
            }

            for c in container_indices.iter().take(num_indices as usize) {
                let container = containers.get_mut(c).unwrap();
                container.crc = data.read_i32();
            }

            for c in container_indices.iter().take(num_indices as usize) {
                let container = containers.get_mut(c).unwrap();
                container.version = data.read_i32();
            }

            let mut container_index_counts = HashMap::<u32, u16>::new(); 

            for c in container_indices.iter().take(num_indices as usize) {
                container_index_counts.insert(*c, data.read_u16());
            }

            for c in container_indices.iter().take(num_indices as usize) {
                let container = containers.get_mut(c).unwrap();
                
                for f in 0..(*container_index_counts.get(c).unwrap() as usize){
                    container.file_indices.push((data.read_u16() as u32) + match f {
                        0 => 0,
                        _ => container.file_indices[f - 1]
                    });

                    container.file_containers.insert(container.file_indices[f], IdxFileContainer::new());
                }
            }

            if whirlpool {
                for (container_index, container_id) in container_indices.iter().enumerate() {
                    for file_index in 0..containers.get(&(container_index as u32)).unwrap().file_containers.len() {
                        let file_id = containers.get(&container_id).unwrap().file_indices[file_index];
                        
                        containers.get_mut(&container_id).unwrap()
                        .file_containers.get_mut(&file_id).unwrap()
                        .version = file_hashes.get(&container_id).unwrap()[file_id as usize];
                    }
                }
            }

            if files_named {
                for c in container_indices.iter().take(num_indices as usize) {
                    let container = containers.get_mut(c).unwrap();

                    for f in 0..(container.file_indices.len()) {
                        let file = container.file_containers.get_mut(&container.file_indices[f]).unwrap();
                        file.name_hash = data.read_u32();
                    }
                }
            }


            Self {
                crc,
                protocol,
                revision,
                container_indices,
                containers,
                named_files: files_named,
                whirlpool
            }
        }
    }
}

#[derive(Default)]
pub struct IdxContainer {
    pub version: i32,
    name_hash: u32,
    pub crc: i32,
    file_indices: Vec<u32>,
    file_containers: HashMap<u32, IdxFileContainer>
}

impl IdxContainer {
    pub fn new() -> Self {
        Self::default()
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct IdxFileContainer {
    version: u8,
    name_hash: u32,
    crc: i32,
    data: Vec<u8>
}

impl IdxFileContainer {
    pub fn new() -> Self {
        Self::default()
    }
}