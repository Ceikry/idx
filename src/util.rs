use std::{sync::{Arc, Mutex}, collections::HashMap, fs::File, io::{Read, BufReader}};
use bzip2::bufread::BzDecoder;
use databuffer::DataBuffer;
use crate::{Cache, CacheIndex};

type ParserFun<T> = fn(DataBuffer) -> T;

pub trait DefParser {
    fn parse_bytes(bytes: Vec<u8>) -> Self where Self: Sized {
        DefParser::parse_buff(DataBuffer::with_vec(bytes))
    }

    fn parse_buff(buffer: DataBuffer) -> Self;
}

/**
  The [`DefProvider`] is going to be what you'll primarily use to implement definition decoders and things along those lines.

  You will use the [`DefProvider::with(cache, index)`] method to construct a definition provider, along with your definition type.

  Let's say, for example, we had the below definition:

  ```ignore
  #[derive(Default)]
  struct DummyDefinition {
      dummy_int: u32,
      dummy_str: String
  }
  ```

  Which resides in index 1. You would implement your decoder:

  ```ignore
  impl DefParser for DummyDefinition {
    fn parse_buff(buffer: DataBuffer) -> Self {
        let mut def = DummyDefinition::default();

        let opcode: u8;

        loop {
            opcode = buffer.read_u8();

            match opcode {
                0 -> break,
                1 -> def.dummy_int = buffer.read_u32(),
                2 -> def.dummy_str = buffer.read_ntstr()
            }
        }

        return def;
    }
  }
  ```

  You would then create a definition provider like so: 

  ```ignore
  use idx::*;
  use idx::util::*;
  use std::sync::{Arc,Mutex};

  let cache = Arc::from(Mutex::from(Cache::from_path("test_cache")));

  let dummy_def_provider = DefProvider::<DummyDefinition>::with(cache, 1);

  let definition = dummy_def_provider.get_def(&3, &1); //returns the parsed definition from file 1 of archive 3.

  ```

  It is additionally recommended to make some additional trait that can turn, for example, and item ID into the appropriate archive and file IDs

  I would also recommend using [`ContainerIdProvider`] as the type to be passed for the ID, as it can accept both u32 and String. But this is up to you.

  ```ignore
  pub trait IdFetch {
      type DefType;

      fn for_id(id: u32) -> &Self::DefType;
  }

  impl IdFetch for DefProvider<DummyDefinition> {
      type DefType = DummyDefinition;

      fn for_id(id: u32) -> &DummyDefinition {
          let archive = id >> 8;
          let file = id & 0xff;

          return self.get_def(&archive, &file, id);
      }
  }
  ```
 */
pub struct DefProvider<T> {
    pub file_provider: FileProvider,
    pub index: u32,
    pub parser: Option<ParserFun<T>>,
    def_cache: HashMap<u32, T>
}

impl <T: DefParser> DefProvider<T> {
    pub fn with(cache: &Arc<Mutex<Cache>>, index: u32) -> Self {
        Self {
            file_provider: FileProvider::from(cache),
            index,
            parser: Some(T::parse_buff),
            def_cache: HashMap::new()
        }
    }

    pub fn get_def(&mut self, archive: &dyn ContainerIdProvider, file: &dyn ContainerIdProvider, id: u32) -> &T {
        if self.def_cache.contains_key(&id) {
            return self.def_cache.get(&id).unwrap();
        }

        self.file_provider.index(self.index);
        self.file_provider.archive(archive);

        let data = self.file_provider.request(file);

        let parse = self.parser.unwrap();

        let def = parse(data);

        self.def_cache.insert(id, def);

        return self.def_cache.get(&id).unwrap();
    }
}

/**
  The FileProvider is the primary method of retrieving raw data from the cache. 

  In order to function correctly, an index, archive and file ID must be supplied.

  The index is type [`usize`], and the archive and file ID can either be a u32 reference (&[`u32`]) or a String reference (&[`String`]).
  
  ```no_run
  use std::{sync::{Arc, Mutex}};
  use idx::util::FileProvider;
  use idx::Cache;

  let cache = Arc::from(Mutex::from(Cache::from_path("test_cache").unwrap()));
  let mut data_provider = FileProvider::from(&cache);
  
  data_provider.index(19).archive(&6);
  let data = data_provider.request(&17); //Returns the raw data for file 17 in archive 6 of index 19.

  assert_ne!(0, data.len());
  ```
*/
pub struct FileProvider {
    cache: Arc<Mutex<Cache>>,
    index: u32,
    archive: u32,
    data_file: Arc<Mutex<BufReader<File>>>,
    keys: Vec<i64>
}

impl FileProvider {
    pub fn from(cache: &Arc<Mutex<Cache>>) -> Self {
        let dfile = match cache.lock() {
            Ok(n) => n.data_file.clone(),

            Err(e) => {
                panic!("Unable to lock cache: {}", e);
            }
        };

        Self {
            cache: cache.clone(),
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
        if self.index == 0 {
            self.archive = archive.get_id(None);
            println!("WARNING: archive was set before the index was! IDX: {}, ARCHIVE: {}. This will break archive access via name hashes!", self.index, self.archive);
        }

        {
            let mut _cache = self.cache.lock().unwrap();
            let index = _cache.index(self.index as usize).unwrap();
            self.archive = archive.get_id(Some(index));
        }
        self
    }

    pub fn with_keys(&mut self, keys: Vec<i64>) {
        self.keys = keys
    }

    pub fn request(&mut self, file: &dyn ContainerIdProvider) -> DataBuffer {
        let file_id = file.get_id(None);

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
            file_data
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
                            DataBuffer::new()
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
            if let Some(file_container) = archive.file_containers.get_mut(&file_info[0]) {
                file_container.data = container_data;
            }
        } else {
            let mut file_sizes = Vec::<i32>::new();
            for _ in 0..(num_loops as usize) {
                let mut offset = 0_i32;
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
                for file_index in &file_info {
                    data_read += buffer.read_i32();

                    match archive.file_containers.get_mut(file_index) {
                        Some(n) => {
                            n.data.append(&mut container_data[(offset as usize)..((offset + data_read) as usize)].to_vec())
                        },
                        None => {
                            println!("Unknown file id: {}", file_index);
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
            file_info.push(*file);
        }

        file_info
    }
}

pub trait ContainerIdProvider {
    fn get_id(&self, _: Option<&mut CacheIndex>) -> u32;
}

impl ContainerIdProvider for String {
    fn get_id(&self, idx: Option<&mut CacheIndex>) -> u32 {
        let hash = get_name_hash(&self);

        if let Some(index) = idx {
            index.get_container_by_name_hash(hash)
        } else {
            hash
        }
    }
}

impl ContainerIdProvider for u32 {
    fn get_id(&self, _: Option<&mut CacheIndex>) -> u32 {
        *self
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
    let mut data = DataBuffer::with_vec(packed_data);
    let mut unpacked = Vec::<u8>::new();

    if data.len() == 0 {
        return Some(Vec::new());
    }

    let compression = data.read_u8();
    let container_size = data.read_u32();

    if container_size > 5000000 {
        println!("Invalid container size! {}", container_size);
        None
    } else {
        match compression {
            0 => { //Uncompressed
                let trim_at = data.get_rpos();
                let mut raw = data.deconstruct();

                raw.drain(..trim_at);
                Some(raw)
            },

            1 => { //Bzip2 (supposedly)
                let decompressed_size = data.read_u32();
                let trim_at = data.get_rpos() - 4;

                let mut trimmed_data = data.deconstruct();
                trimmed_data.drain(..trim_at);

                //Re-add header jagex strips.
                trimmed_data[0] = b'B';
                trimmed_data[1] = b'Z';
                trimmed_data[2] = b'h';
                trimmed_data[3] = b'1';

                match BzDecoder::new(&trimmed_data[..]).read_to_end(&mut unpacked) {
                    Ok(_) => {},
                    Err(e) => {
                        println!("Bzip2 Decompression Error: {}", e);
                    }
                }

                assert_eq!(decompressed_size, unpacked.len() as u32);
                Some(unpacked)
            },

            _ => { //DEFLATE/Gzip/Zip
                let decompressed_size = data.read_u32();
                data.set_rpos(data.get_rpos() + 10);
                let trim_at = data.get_rpos();

                let mut trimmed_data = data.deconstruct();
                trimmed_data.drain(..trim_at);

                unpacked = match inflate::inflate_bytes(&trimmed_data) {
                    Ok(n) => n,
                    Err(e) => {
                        println!("Error deflating gzip-compressed cache data: {}", e);
                        return None;
                    }
                };

                assert_eq!(decompressed_size, unpacked.len() as u32);
                Some(unpacked)
            }
        }
    }
}
