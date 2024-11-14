use std::{
    io::{self, IoSlice, SeekFrom},
    path::Path,
};

use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};
use unico::asym::AsymWait;

#[allow(dead_code)]
/// The backend which takes care of the actual file operations for the FAT
/// filesystem images.
pub trait Backend: Sized + io::Read + io::Write + io::Seek {
    /// Open an existing image file. The file must already exist.
    fn open<P: AsRef<Path> + Send>(path: P) -> io::Result<Self>;
    /// Create a new image file with the given size, then initialize it with the
    /// given closure. Truncate the file if it already exists.
    fn create<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self>;
    /// Create a new image file with the given size, then initialize it with the
    /// given closure. Returns error if the file already exists.
    fn create_new<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self>;

    /// Open an existing image file, if it doesn't exist, create a new one and
    /// initialize it with the given closure.
    fn open_or_create<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        let path = path.as_ref();
        Self::open(path).or_else(|_| Self::create_new(path, size, init))
    }

    /// Perform a real flush operation.
    fn real_flush(&mut self) -> io::Result<()> {
        self.flush()
    }
}

/// A backend using [`tokio::fs::File`] and unico, must be used in
/// [`sync`](unico::async::sync), and tokio runtime.
pub struct UnicoBackend {
    file: File,
}

impl Backend for UnicoBackend {
    fn open<P: AsRef<Path> + Send>(path: P) -> io::Result<Self> {
        File::options()
            .read(true)
            .write(true)
            .create(false)
            .open(path)
            .wait()
            .map(|file| Self { file })
    }

    fn create<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .wait()
            .and_then(|file| file.set_len(size).wait().map(|_| Self { file }))
            .and_then(|mut backend| init(&mut backend).map(|_| backend))
    }

    fn create_new<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        File::create_new(path)
            .wait()
            .and_then(|file| file.set_len(size).wait().map(|_| Self { file }))
            .and_then(|mut backend| init(&mut backend).map(|_| backend))
    }
}

impl io::Read for UnicoBackend {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf).wait()
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.file.read_to_end(buf).wait()
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.file.read_to_string(buf).wait()
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.file.read_exact(buf).wait().map(|_| ())
    }
}

impl io::Write for UnicoBackend {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf).wait()
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.file.write_vectored(bufs).wait()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush().wait()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.file.write_all(buf).wait().map(|_| ())
    }
}

impl io::Seek for UnicoBackend {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos).wait()
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.file.rewind().wait().map(|_| ())
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        self.file.stream_position().wait()
    }
}

/// A synchronous backend using [`std::fs::File`].
pub type SyncBackend = std::fs::File;

impl Backend for SyncBackend {
    fn open<P: AsRef<Path> + Send>(path: P) -> io::Result<Self> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(path)
    }

    fn create<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .and_then(|file| file.set_len(size).map(|_| file))
            .and_then(|mut file| init(&mut file).map(|_| file))
    }

    fn create_new<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        std::fs::File::create_new(path)
            .and_then(|file| file.set_len(size).map(|_| file))
            .and_then(|mut file| init(&mut file).map(|_| file))
    }
}

mod cached;
pub use cached::*;

mod rw_count {
    use sha2::digest::consts::P1000000;

    use super::Backend;

    pub struct RWCount<B: Backend> {
        pub backend: B,
        pub read_count: u64,
        pub write_count: u64,
    }

    impl<B: Backend> RWCount<B> {
        pub fn new(backend: B) -> Self {
            Self {
                backend,
                read_count: 0,
                write_count: 0,
            }
        }
    }

    impl<B: Backend> Backend for RWCount<B> {
        fn open<P: AsRef<std::path::Path> + Send>(path: P) -> std::io::Result<Self> {
            B::open(path).map(Self::new)
        }

        fn create<P: AsRef<std::path::Path> + Send, F: FnOnce(&mut Self) -> std::io::Result<()>>(
            path: P,
            size: u64,
            init: F,
        ) -> std::io::Result<Self> {
            let mut init_called = false;
            let mut backend = B::create(path, size, |_| {
                init_called = true;
                Ok(())
            }).map(Self::new)?;

            if init_called {
                init(&mut backend)?;
            }

            Ok(backend)
        }

        fn create_new<P: AsRef<std::path::Path> + Send, F: FnOnce(&mut Self) -> std::io::Result<()>>(
            path: P,
            size: u64,
            init: F,
        ) -> std::io::Result<Self> {
            let mut init_called = false;
            let mut backend = B::create_new(path, size, |_| {
                init_called = true;
                Ok(())
            }).map(Self::new)?;

            if init_called {
                init(&mut backend)?;
            }

            Ok(backend)
        }

        fn real_flush(&mut self) -> std::io::Result<()> {
            self.backend.real_flush()
        }
    }

    impl<B: Backend> std::io::Read for RWCount<B> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read_count += 1;
            self.backend.read(buf)
        }
    }

    impl<B: Backend> std::io::Write for RWCount<B> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.write_count += 1;
            self.backend.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.backend.flush()
        }
    }

    impl<B: Backend> std::io::Seek for RWCount<B> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.backend.seek(pos)
        }

        fn stream_position(&mut self) -> std::io::Result<u64> {
            self.backend.stream_position()
        }
    }

    impl<B: Backend> Drop for RWCount<B> {
        fn drop(&mut self) {
            println!(
                "Read count: {}, Write count: {}",
                self.read_count, self.write_count
            );
        }
    }
}

pub use rw_count::RWCount;
