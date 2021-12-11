use std::{sync::{Arc, Mutex}, collections::HashMap, fs::File, io::Read};
use bzip2::bufread::BzDecoder;
use databuffer::DataBuffer;
use crate::Cache;

type ParserFun<T> = fn(DataBuffer) -> T;

pub trait DefParser {
    fn parse_bytes(bytes: Vec<u8>) -> Self where Self: Sized {
        return DefParser::parse_buff(DataBuffer::from_bytes(&bytes));
    }

    fn parse_buff(buffer: DataBuffer) -> Self;
}

pub struct DefProvider<T> {
    pub cache: Arc<Mutex<Cache>>,
    pub index: u32,
    pub parser: Option<ParserFun<T>>,
    def_cache: HashMap<u32, T>
}

impl <T: DefParser> DefProvider<T> {
    pub fn with(cache: Arc<Mutex<Cache>>, index: u32) -> Self {
        Self {
            cache: cache.clone(),
            index,
            parser: Some(T::parse_buff),
            def_cache: HashMap::new()
        }
    }

    pub fn get_def(&mut self, archive: &dyn ContainerIdProvider, file: &dyn ContainerIdProvider, id: u32) -> &T {
        if self.def_cache.contains_key(&id) {
            return self.def_cache.get(&id).unwrap();
        }

        let mut data_provider = FileProvider::from(self.cache.clone());

        data_provider.index(self.index);
        data_provider.archive(&archive.get_id());
        let data = data_provider.request(&file.get_id());

        let parse = self.parser.unwrap();

        let def = parse(data);

        self.def_cache.insert(id, def);

        return self.def_cache.get(&id).unwrap();
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

fn get_name_hash(name: &str) -> u32 {
    let name_clean = name.to_lowercase();

    let mut hash = 0;

    for char in name_clean.into_bytes() {
        hash = (char as u32) + ((hash << 5) - hash);
    }

    hash
}

pub(crate) fn decompress_container_data(packed_data: Vec<u8>) -> Option<Vec<u8>> {
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
