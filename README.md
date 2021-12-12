[![Crates.io](https://img.shields.io/crates/v/idx-rs.svg)](https://crates.io/crates/idx-rs)
[![Documentation](https://docs.rs/idx-rs/badge.svg)](https://docs.rs/idx-rs)
[![Discord Chat](https://img.shields.io/discord/918184459315056683.svg)](https://discord.gg/pMnw9H2Art)  

![idx-github](https://user-images.githubusercontent.com/61421472/145660771-6eb75b4c-10f0-4cfc-b6e1-ca36d1d16f23.png)
*This image proudly made in GIMP*

## License

Licensed under GNU GPL, Version 3.0, ([LICENSE-GPL3](LICENSE-GPL3) or https://choosealicense.com/licenses/gpl-3.0/)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the GPL-3.0 license, will be incorporated
into the project under the full terms of the GPL-3.0 license.

### Where can I get a cache?
I can not, will not, and don't want to provide a copy of any cache owned by Jagex. These are copyrighted
assets and I do not encourage the violation of their copyright. These are up to you to source.
A good resource is the [OpenRS2 Archive](https://archive.openrs2.org/).

### Quick Start

IDX is actually relatively straightforward to use. If you just want to get a working example
spun up as quick as possible, below is an example configuration that should get you going in no time.

Note that `test_cache` in the below example should be replaced with the path to your IDX-formatted cache.


```rs
use std::sync::{Arc, Mutex};

use idx::*;
use idx::util::*;

fn main() {
    let cache = Arc::from(Mutex::from(Cache::from_path("test_cache")));

    let data_provider = FileProvider::from(&cache);
    data_provider.index(19).archive(&1);

    let mut data: DataBuffer = data_provider.request(&0); //This will return a DataBuffer containing the data from File 0, Archive 1, Index 19.
}
```

IDX's `Cache` struct is designed to be wrapped in an `Arc<Mutex<Cache>>` so that multiple references to it can be created at once. 
Both the `FileProvider` and `DefProvider` leverage this to allow creation of multiple simultaneous file/definition providers. 

For more information on `FileProvider` and `DefProvider` check the documentation on docs.rs, specifically: [FileProvider](https://docs.rs/idx-rs/latest/idx/util/struct.FileProvider.html) and [DefProvider](https://docs.rs/idx-rs/latest/idx/util/struct.DefProvider.html).

### Benchmarks
IDX is very fast for what it has to do. Some of the speed obviously depends on whether you are using an SSD or HDD, but generally speaking, the speeds are substantial. 
Due to benchmarking thanks to [Criterion](https://crates.io/crates/criterion), I am able to provide the below graphs benchmarking reading random files from Index 19.
