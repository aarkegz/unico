use super::Backend;

pub struct RWCount<B: Backend> {
    pub backend: B,
    pub read_count: u64,
    pub write_count: u64,
    pub seek_count: u64,
}

impl<B: Backend> RWCount<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            read_count: 0,
            write_count: 0,
            seek_count: 0,
        }
    }
}

impl<B: Backend> Backend for RWCount<B> {
    fn open<P: AsRef<std::path::Path> + Send>(path: P) -> std::io::Result<Self> {
        B::open(path).map(Self::new)
    }

    fn create<
        P: AsRef<std::path::Path> + Send,
        F: FnOnce(&mut Self) -> std::io::Result<()>,
    >(
        path: P,
        size: u64,
        init: F,
    ) -> std::io::Result<Self> {
        let mut init_called = false;
        let mut backend = B::create(path, size, |_| {
            init_called = true;
            Ok(())
        })
        .map(Self::new)?;

        if init_called {
            init(&mut backend)?;
        }

        Ok(backend)
    }

    fn create_new<
        P: AsRef<std::path::Path> + Send,
        F: FnOnce(&mut Self) -> std::io::Result<()>,
    >(
        path: P,
        size: u64,
        init: F,
    ) -> std::io::Result<Self> {
        let mut init_called = false;
        let mut backend = B::create_new(path, size, |_| {
            init_called = true;
            Ok(())
        })
        .map(Self::new)?;

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
        self.seek_count += 1;
        self.backend.seek(pos)
    }

    fn stream_position(&mut self) -> std::io::Result<u64> {
        self.backend.stream_position()
    }
}

impl<B: Backend> Drop for RWCount<B> {
    fn drop(&mut self) {
        println!(
            "Read count: {}, Write count: {}, Seek count: {}",
            self.read_count, self.write_count, self.seek_count
        );
    }
}
