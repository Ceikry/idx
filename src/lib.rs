use core::panic;
use std::{io::{Seek, SeekFrom, Read}, fs::{File, OpenOptions}, path::PathBuf, collections::HashMap, sync::{Arc, Mutex, MutexGuard}};

use bzip2::bufread::BzDecoder;
use databuffer::DataBuffer;

type IdxFileOpt<'a> = Option<&'a mut CacheIndex>;

pub struct Cache {
    pub data_file: Arc<Mutex<File>>,
    indices: HashMap<u8, CacheIndex>
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
            Ok(n) => Arc::from(Mutex::from(n)),
            Err(e) => {
                println!("Failed opening data file: {:?}, Error: {}", &path_buff, e);
                return None;
            }
        };

        let num_files = info_file.seek(SeekFrom::End(0)).unwrap() / 6;
        let _ = info_file.seek(SeekFrom::Start(0));

        let mut info = CacheIndex::from(255, 500000, info_file, IdxContainerInfo::new());
        let mut indices = HashMap::<u8, CacheIndex>::new();

        for i in 1..num_files {
            path_buff.clear();
            path_buff.push(path);
            path_buff.push(format!("main_file_cache.idx{}",&i));

            let file = match OpenOptions::new().read(true).open(&path_buff) {
                Ok(n) => n,
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

        Some(Self {
            data_file: data_file,
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
    file: File,
    max_container_size: u32,
    container_info: IdxContainerInfo
}

impl CacheIndex {
    fn from(file_id: u8, max_size: u32, file: File, container_info: IdxContainerInfo) -> Self {
        Self {
            file_id,
            max_container_size: max_size,
            file,
            container_info
        }
    }

    fn container_data(&mut self, mut data_file: MutexGuard<File>, container_id: u32) -> Option<Vec<u8>> {
        let mut file_buff: [u8; 520] = [0; 520];
        let mut data: [u8;6] = [0; 6];
        let _ = self.file.seek(SeekFrom::Start(6 * (container_id as u64)));
        let _ = match self.file.read(&mut data) {
            Ok(_) => {}
            Err(e) => {
                println!("Error reading from info file: {}", e);
            }
        };

        let container_size = ((data[2] & 0xff) as u32) + ((((0xff & data[0]) as u32) << 16) + (((data[1] as u32) << 8) & 0xff00));
        let mut sector = (((data[3] & 0xff) as i32) << 16) - (-((0xff & data[4] as i32) << 8) - (data[5] as i32 & 0xff)); 

        if container_size > self.max_container_size {
            println!("Container Size greater than Max Container Size! {} > {}", container_size, self.max_container_size);
            None
        } else {
            if sector <= 0 {
                println!("Sector <= 0! {}", sector);
                None
            } else {
                let mut container_data = Vec::<u8>::new();

                let mut data_read_count = 0;
                let mut part = 0;

                while container_size > data_read_count {
                    if sector == 0 {
                        println!("Sector == 0!");
                        return None;
                    }

                    let _ = data_file.seek(SeekFrom::Start(520 * (sector as u64)));

                    let mut data_to_read = container_size - data_read_count;

                    if data_to_read > 512 {
                        data_to_read = 512;
                    }

                    let _ = data_file.read(&mut file_buff);

                    let current_container_id = (0xff & file_buff[1] as u32) + ((0xff & file_buff[0] as u32) << 8);
                    let current_part = ((0xff & file_buff[2] as u32) << 8) + (0xff & file_buff[3] as u32);
                    let next_sector = (0xff & file_buff[6] as u32) + ((0xff & file_buff[5] as u32) << 8) + ((0xff & file_buff[4] as u32) << 16);
                    let current_idx_file_id = 0xff & file_buff[7] as u32;

                    if container_id != (current_container_id as u32) || current_part != part || self.file_id != (current_idx_file_id as u8) {
                        println!("Multipart failure! {} != {} || {} != {} || {} != {}", container_id, current_container_id, current_part, part, self.file_id, current_idx_file_id);
                        return None;
                    }

                    for i in 0..data_to_read {
                        container_data.push(file_buff[(8 + i as usize)]);
                        data_read_count += 1;
                    }

                    part += 1;
                    sector = next_sector as i32;
                }

                Some(container_data)
            }
        }
    }

    pub fn get_total_files(&mut self) -> u32 {
        self.container_info.container_indices.sort();

        let last_archive_id = self.container_info.container_indices.last().unwrap().clone();
        let last_archive = self.container_info.containers.get(&last_archive_id).unwrap();

        let last_archive_file_amount = last_archive.file_indices.len();
        let other_file_amounts = (self.container_info.container_indices.len() - 1) * 256;
        
        return (last_archive_file_amount + other_file_amounts) as u32;
    }
}

pub struct IdxContainerInfo {
    protocol: u8,
    revision: u32,
    container_indices: Vec<u32>,
    containers: HashMap<u32, IdxContainer>,
    named_files: bool,
    whirlpool: bool
}

impl IdxContainerInfo {
    pub fn new() -> Self {
        Self {
            protocol: 0,
            revision: 0,
            container_indices: Vec::new(),
            containers: HashMap::new(),
            named_files: false,
            whirlpool: false,
        }
    }

    pub fn from(packed_data: Vec<u8>) -> Self {
        let mut data = match decompress_container_data(packed_data) {
            Some(n) => DataBuffer::from_bytes(&n),
            None => {
                println!("Unable to decompress container data.");
                return Self::new();
            }
        };

        let protocol = data.read_u8();
        
        if protocol != 5 && protocol != 6 {
            println!("Invalid protocol while parsing container info: {}", protocol);
            return Self::new();
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
                    _ => container_indices.last().unwrap().clone()
                });

                containers.insert(container_indices.last().unwrap().clone(), IdxContainer::new());
            }

            if files_named {
                for i in 0..(num_indices as usize) {
                    containers.get_mut(&container_indices[i]).unwrap().name_hash = data.read_u32();
                }
            }

            let mut file_hashes: HashMap<u32, [u8;64]> = HashMap::new();

            //NOTE: This is not handled correctly.
            if whirlpool {
                for i in 0..(num_indices as usize) {
                    let mut buf: [u8; 64] = [0; 64];
                    let _ = data.read(&mut buf);
                    file_hashes.insert(container_indices[i].clone(), buf);
                }
            }

            for i in 0..(num_indices as usize) {
                let container = containers.get_mut(&container_indices[i]).unwrap();
                container.crc = data.read_i32();
            }

            for i in 0..(num_indices as usize) {
                let container = containers.get_mut(&container_indices[i]).unwrap();
                container.version = data.read_i32();
            }

            let mut container_index_counts = HashMap::<u32, u16>::new(); 

            for i in 0..(num_indices as usize) {
                container_index_counts.insert(container_indices[i].clone(), data.read_u16());
            }

            for i in 0..(num_indices as usize) {
                let container = containers.get_mut(&container_indices[i]).unwrap();
                
                for f in 0..(container_index_counts.get(&container_indices[i]).unwrap().clone() as usize){
                    container.file_indices.push((data.read_u16() as u32) + match f {
                        0 => 0,
                        _ => container.file_indices[f - 1]
                    });

                    container.file_containers.insert(container.file_indices[f].clone(), IdxFileContainer::new());
                }
            }

            //NOTE: This is not handled correctly. I didn't see the need to handle this correctly when initially written.
            if whirlpool {
                for container_index in 0..container_indices.len() {
                    for file_index in 0..containers.get(&(container_index as u32)).unwrap().file_containers.len() {
                        let container_id = container_indices[container_index];
                        let file_id = containers.get(&container_id).unwrap().file_indices[file_index];
                        
                        containers.get_mut(&container_id).unwrap()
                        .file_containers.get_mut(&file_id).unwrap()
                        .version = file_hashes.get(&container_id).unwrap()[file_id as usize];
                    }
                }
            }

            if files_named {
                for i in 0..(num_indices as usize) {
                    let container = containers.get_mut(&container_indices[i]).unwrap();

                    for f in 0..(container.file_indices.len()) {
                        let file = container.file_containers.get_mut(&container.file_indices[f]).unwrap();
                        file.name_hash = data.read_u32();
                    }
                }
            }


            Self {
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

pub struct IdxContainer {
    version: i32,
    name_hash: u32,
    crc: i32,
    file_indices: Vec<u32>,
    file_containers: HashMap<u32, IdxFileContainer>
}

impl IdxContainer {
    pub fn new() -> Self {
        Self {
            version: -1,
            name_hash: 0,
            crc: -1,
            file_indices: Vec::new(),
            file_containers: HashMap::new()
        }
    }
}

pub struct IdxFileContainer {
    version: u8,
    name_hash: u32,
    crc: i32,
    data: Vec<u8>
}

impl IdxFileContainer {
    pub fn new() -> Self {
        Self {
            version: 0,
            name_hash: 0,
            crc: -1,
            data: Vec::new()
        }
    }
}

fn get_name_hash(name: &str) -> u32 {
    let name_clean = name.to_lowercase();

    let mut hash = 0;

    for char in name_clean.into_bytes() {
        hash = (char as u32) + ((hash << 5) - hash);
    }

    hash
}

fn decompress_container_data(packed_data: Vec<u8>) -> Option<Vec<u8>> {
    let mut data = DataBuffer::from_bytes(&packed_data);
    let mut unpacked = Vec::<u8>::new();

    if packed_data.is_empty() {
        return Some(Vec::new());
    }

    let compression = data.read_u8();
    let container_size = data.read_u32();

    if container_size > 5000000 {
        println!("Invalid container size! {}", container_size);
        return None;
    } else {
        match compression {
            0 => { //Uncompressed
                for _ in 0..container_size {
                    unpacked.push(data.read_u8());
                }
        
                return Some(unpacked);
            },

            1 => { //Bzip2 (supposedly)
                let decompressed_size = data.read_u32();
                let mut current_index: usize = 0;
                let trim_at = data.get_rpos() - 4;

                let mut trimmed_data = data.to_bytes();
                trimmed_data.retain(|_| {
                    current_index +=1;
                    current_index - 1 >= trim_at
                });

                //Re-add header jagex strips.
                trimmed_data[0] = 'B' as u8;
                trimmed_data[1] = 'Z' as u8;
                trimmed_data[2] = 'h' as u8;
                trimmed_data[3] = '1' as u8;

                match BzDecoder::new(&trimmed_data[..]).read_to_end(&mut unpacked) {
                    Ok(_) => {},
                    Err(e) => {
                        println!("Bzip2 Decompression Error: {}", e);
                    }
                }

                assert_eq!(decompressed_size, unpacked.len() as u32, "packed size: {}, decompressed correct: {}, current decompressed: {}", packed_data.len(), decompressed_size, unpacked.len());
                return Some(unpacked);
            },

            _ => { //DEFLATE/Gzip/Zip
                let decompressed_size = data.read_u32();
                let mut current_index: usize = 0;
                data.set_rpos(data.get_rpos() + 10);
                let trim_at = data.get_rpos();

                let mut trimmed_data = data.to_bytes();
                trimmed_data.retain(|_| {
                    current_index +=1;
                    current_index - 1 >= trim_at
                });

                unpacked = match inflate::inflate_bytes(&trimmed_data) {
                    Ok(n) => n,
                    Err(e) => {
                        println!("Error deflating gzip-compressed cache data: {}", e);
                        return None;
                    }
                };

                assert_eq!(decompressed_size, unpacked.len() as u32, "packed size: {}, trimmed size: {}, decompressed correct: {}, current decompressed: {}", packed_data.len(), trimmed_data.len(), decompressed_size, unpacked.len());
                return Some(unpacked);
            }
        }
    }
}

pub struct FileProvider {
    cache: Arc<Mutex<Cache>>,
    index: u32,
    archive: u32,
    data_file: Arc<Mutex<File>>,
    keys: Vec<i64>
}

impl FileProvider {
    pub fn from(cache: Arc<Mutex<Cache>>) -> Self {
        let dfile = match cache.lock() {
            Ok(n) => match n.data_file.lock() {
                Ok(n) => match n.try_clone() {
                    Ok(n) => Arc::from(Mutex::from(n)),
                    Err(e) => {
                        panic!("Unable to obtain new file reference: {}", e);
                    }
                }

                Err(e) => {
                    panic!("Unable to lock data file reference: {}", e);
                }
            }

            Err(e) => {
                panic!("Unable to lock cache: {}", e);
            }
        };

        Self {
            cache,
            index: 0,
            archive: 0,
            data_file: dfile,
            keys: Vec::new()
        }
    }

    pub fn index(&mut self, index: u32) -> &mut Self {
        self.index = index;
        self
    }

    pub fn archive(&mut self, archive: &dyn ContainerIdProvider) -> &mut Self {
        self.archive = archive.get_id();
        self
    }

    pub fn with_keys(&mut self, keys: Vec<i64>) {
        self.keys = keys
    }

    pub fn request(&mut self, file: &dyn ContainerIdProvider) -> DataBuffer {
        let file_id = file.get_id();

        let file_data = match self.cache.lock() {
            Ok(mut n) => match n.index(self.index as usize) {
                Some(s) => match s.container_info.containers.get(&self.archive) {
                    Some(c) => match c.file_containers.get(&file_id) {
                        Some(n) => DataBuffer::from_bytes(&n.data),
                        None => DataBuffer::new()
                    }
                    None => {
                        println!("Invalid archive supplied?");
                        return DataBuffer::new();
                    }
                },
                None => {
                    panic!("Index has no containers?");
                }
            },
            Err(_) => {
                panic!("Unable to lock cache!");
            }
        };

        if file_data.len() != 0 {
            return file_data;
        } else {
            self.load_requested_container_files();

            match self.cache.lock() {
                Ok(mut n) => match n.index(self.index as usize) {
                    Some(s) => match s.container_info.containers.get(&self.archive) {
                        Some(c) => match c.file_containers.get(&file_id) {
                            Some(n) => DataBuffer::from_bytes(&n.data),
                            None => DataBuffer::new()
                        }
                        None => {
                            println!("Invalid archive supplied?");
                            return DataBuffer::new()
                        }
                    },
                    None => {
                        panic!("Index has no containers?");
                    }
                },
                Err(_) => {
                    panic!("Unable to lock cache!");
                }
            }
        }
    }

    fn load_requested_container_files(&mut self) {
        let container_data = self.get_requested_container_data();
        let file_info = self.get_container_file_info();

        println!("FILE NUM: {}", file_info.len());

        let mut read_pos = container_data.len() - 1;
        let num_loops = container_data[read_pos];

        read_pos -= (num_loops as usize) * (file_info.len() * 4);

        let mut buffer = DataBuffer::from_bytes(&container_data);
        buffer.set_rpos(read_pos as usize);

        let mut cache = match self.cache.lock() {
            Ok(n) => n,
            Err(_) => return
        };

        let index = match cache.index(self.index as usize) {
            Some(n) => n,
            None => return
        };

        let archive = match index.container_info.containers.get_mut(&self.archive) {
            Some(n) => n,
            None => return
        };

        if file_info.len() == 1 {
            match archive.file_containers.get_mut(&file_info[0]) {
                Some(n) => n.data = container_data,
                None => return
            }
        } else {
            println!("{} - {} - {}", buffer.len(), num_loops, read_pos);
            let mut file_sizes = Vec::<i32>::new();
            for _ in 0..(num_loops as usize) {
                let mut offset = 0 as i32;
                for file_index in 0..(file_info.len() as usize){
                    offset += buffer.read_i32();
                    if file_sizes.len() == file_index {
                        file_sizes.push(offset);
                    } else {
                        file_sizes[file_index] += offset;
                    }
                }
            }

            buffer.set_rpos(read_pos);

            let mut offset = 0;
            for _ in 0..(num_loops as usize) {
                let mut data_read = 0;
                for file_index in 0..(file_info.len()) {
                    data_read += buffer.read_i32();

                    match archive.file_containers.get_mut(&file_info[file_index]) {
                        Some(n) => {
                            n.data.append(&mut container_data[(offset as usize)..((offset + data_read) as usize)].to_vec())
                        },
                        None => {
                            println!("Unknown file id: {}", file_info[file_index]);
                            continue;
                        }
                    }

                    offset += data_read;
                }
            }
        }
    }

    fn get_requested_container_data(&mut self) -> Vec<u8> {
        let mut _cache = self.cache.lock().unwrap();

        let index = match _cache.index(self.index as usize) {
            Some(n) => n,
            None => {
                return Vec::new();
            }
        };

        let _ = match index.container_data(self.data_file.lock().unwrap(), self.archive) {
            Some(n) => match decompress_container_data(n) {
                Some(n) => return n,
                None => return Vec::new()
            },
            None => return Vec::new()
        };
    }

    fn get_container_file_info(&mut self) -> Vec<u32> {
        let mut file_info = Vec::<u32>::new();

        let mut _cache = self.cache.lock().unwrap();

        let index = match _cache.index(self.index as usize) {
            Some(n) => n,
            None => {
                return Vec::new();
            }
        };

        let container = match index.container_info.containers.get(&self.archive) {
            Some(n) => n,
            None => return Vec::new()
        };

        for file in container.file_indices.iter() {
            file_info.push(file.clone());
        }

        file_info
    }
}

pub trait ContainerIdProvider {
    fn get_id(&self) -> u32;
}

impl ContainerIdProvider for str {
    fn get_id(&self) -> u32 {
        return get_name_hash(&self);
    }
}

impl ContainerIdProvider for u32 {
    fn get_id(&self) -> u32 {
        return self.clone();
    }
}
