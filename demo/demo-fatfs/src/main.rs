#![feature(allocator_api)]
#![feature(future_join)]
#![feature(new_uninit)]

mod backend;

use std::{
    alloc::Global,
    io::{Read, Write},
    path::Path,
};

use backend::{Backend, CachedBackend, RWCount, SyncBackend, UnicoBackend};
use fatfs::{FatType, FsOptions};
use futures::{stream::FuturesUnordered, StreamExt};
use rand::{RngCore, SeedableRng};
use sha2::Digest;
use unico::{
    asym::sync,
    context::{boost::Boost, global_resumer},
    stack::global_stack_allocator,
};

// A demo job:
// 1. Create a file system image with a backend.
// 2. Write a file 'metadata.txt' with the following content: The file system
//    image is created at <current time>, with a size of <fs_size> bytes. The
//    random data, with a size of <file_size> bytes, is generated with seed
//    <seed>, and written to 'random.bin'.
// 3. Write a file 'random.bin' with random data.
// 4. Write a file 'zero.bin' with no data.
// 5. Read the file 'random.bin' and check if the data is correct.
fn do_job<B: Backend>(
    output: impl AsRef<Path> + Send,
    fs_size: u64,
    file_size: u64,
    seed: u64,
) -> std::io::Result<()> {
    const SECTOR_SIZE: u64 = 512;
    const BLOCK_SIZE: u64 = 1048576;

    println!("#{}: Enter do_job", seed);

    // 1. Create a file system image with a backend.
    let fs_size = fs_size.next_multiple_of(SECTOR_SIZE);

    let backend = B::create(output, fs_size, |backend| {
        let options = fatfs::FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .total_sectors((fs_size / SECTOR_SIZE) as u32);
        fatfs::format_volume(backend, options)
    })?;

    let fs = fatfs::FileSystem::new(backend, FsOptions::new())?;
    let root_dir = fs.root_dir();

    println!("#{}: File system image created", seed);

    // 2. Write a file 'metadata.txt' with the following content:
    let mut desc_file = root_dir.create_file("metadata.txt").unwrap();
    desc_file.write_all(
        format!(
            "The file system image is created at {}, with a size of {} bytes.\n",
            chrono::Local::now(),
            fs_size
        )
        .as_bytes(),
    )?;
    desc_file.write_all(
        format!(
            "The random data, with a size of {} bytes, is generated with seed {}, and written to 'random.bin'.\n",
            file_size, seed
        )
        .as_bytes(),
    )?;
    drop(desc_file);

    println!("#{}: metadata.txt written", seed);

    // 3. Write a file 'random.bin' with random data.
    let file_size = file_size.next_multiple_of(BLOCK_SIZE);
    let mut random_file = root_dir.create_file("random.bin").unwrap();
    // pcg64 is fast and good enough for this job, it takes around 11% of the total
    // time when the file size is 90MB
    let mut rng = rand_pcg::Pcg64::seed_from_u64(seed);
    let mut buf = vec![0u8; BLOCK_SIZE as usize];
    let mut checksum = sha2::Sha224::new();
    for _ in 0..file_size / BLOCK_SIZE {
        rng.fill_bytes(&mut buf);
        random_file.write_all(&buf)?;
        checksum.update(&buf);
    }
    drop(random_file);
    let expected_checksum = checksum.finalize();

    println!("#{}: random.bin written", seed);

    // 4. Write a file 'zero.bin' with no data.
    let zero_file = root_dir.create_file("zero.bin").unwrap();
    drop(zero_file);

    println!("#{}: zero.bin written", seed);

    // 5. Read the file 'random.bin' and check if the data is correct.
    for _ in 0..256 {
        let mut checksum = sha2::Sha224::new();
        let mut random_file = root_dir.open_file("random.bin").unwrap();
        for _ in 0..file_size / BLOCK_SIZE {
            random_file.read_exact(&mut buf)?;
            checksum.update(&buf);
        }
        let actual_checksum = checksum.finalize();
        assert_eq!(expected_checksum, actual_checksum);
        drop(random_file);
    }

    println!("#{}: random.bin read", seed);

    Ok(())
}

const JOBS: usize = 24;
const FS_SIZE: u64 = 100 * 1024 * 1024;
const FILE_SIZE: u64 = 90 * 1024 * 1024;

const fn fs_size(id: usize, total: usize) -> u64 {
    ((60 + (total - id) * 3) * 1024 * 1024) as u64
}

const fn file_size(id: usize, total: usize) -> u64 {
    ((55 + (total - id) * 3) * 1024 * 1024) as u64
}

fn sync_main<B: Backend>() {
    for i in 0..JOBS {
        do_job::<B>(format!("{}.img", i), fs_size(i, JOBS), file_size(i, JOBS), i as u64).unwrap();
    }
}

fn async_main<B: Backend>() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async move {
        let tasks: Vec<_> = (0..JOBS)
            .map(|i| {
                tokio::spawn(async move {
                    sync(|| {
                        do_job::<B>(
                            format!("{}.img", i),
                            fs_size(i, JOBS),
                            file_size(i, JOBS),
                            i as u64,
                        )
                        .unwrap()
                    })
                    .await;
                })
            })
            .collect();

        for task in tasks {
            task.await.unwrap();
        }
    });
}

fn main() {
    {
        let now = std::time::Instant::now();
        sync_main::<CachedBackend<RWCount<SyncBackend>>>();
        println!("sync_main::<SyncBackend> took {:?}", now.elapsed());
    }
    {
        global_resumer!(Boost);
        global_stack_allocator!(Global);

        let now = std::time::Instant::now();
        async_main::<CachedBackend<RWCount<UnicoBackend>>>();
        println!("sync_main::<UnicoBackend> took {:?}", now.elapsed());
    }
}
