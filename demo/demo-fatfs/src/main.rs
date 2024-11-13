#![feature(allocator_api)]

mod backend;

use std::{
    alloc::Global, fs, io::{Seek, SeekFrom, Write}
};

use backend::{Backend, SyncBackend, UnicoBackend};
use fatfs::{FatType, FsOptions};
use unico::{asym::sync, context::{boost::Boost, global_resumer}, stack::global_stack_allocator};

fn sync_main<B: Backend>() {
    const FILENAME: &'static str = "fat.img";
    const SIZE: u64 = 40 * 1024 * 1024; // 40 MB

    let backend = B::open_or_create(FILENAME, SIZE, |backend| {
        let options = fatfs::FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .total_sectors((SIZE / 512) as u32);
        fatfs::format_volume(backend, options)
    })
    .unwrap();

    let fs = fatfs::FileSystem::new(backend, FsOptions::new()).unwrap();
    let root_dir = fs.root_dir();
    let mut file = root_dir.create_file("1.txt").unwrap();

    file.seek(SeekFrom::End(0)).unwrap();

    let size = root_dir.iter().find(|e| {
        e.as_ref().is_ok_and(|e| e.file_name() == "1.txt")
    }).unwrap().unwrap().len();
    let now_string = chrono::Local::now().to_string();

    file.write_all(format!("Hello, world! {}: 1.txt is {} bytes long.\n", now_string, size).as_bytes()).unwrap();

    drop(file);
    drop(root_dir);
    fs.unmount().unwrap()
}

fn main() {
    #[cfg(feature = "sync")]
    {
        sync_main::<SyncBackend>();
    }
    {

        global_resumer!(Boost);
        global_stack_allocator!(Global);

        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                sync(|| sync_main::<UnicoBackend>()).await
            });
    }
}
