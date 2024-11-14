use std::{
    collections::BTreeMap, io::{self, SeekFrom}, ops::Add, path::Path
};

use super::Backend;

const PAGE_SIZE: usize = 1048576;

pub enum PageType {
    FullPage {
        number: u64,
    },
    PartialPage {
        number: u64,
        offset: usize,
        size: usize,
    },
}

pub struct PageRange {
    pub start: u64,
    pub end: u64,
}

impl PageRange {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }
}

impl Iterator for PageRange {
    type Item = PageType;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start >= self.end {
            return None;
        }

        let start = self.start;
        let number = start / PAGE_SIZE as u64;
        let offset = start % PAGE_SIZE as u64;
        let expected_end = (start + 1).next_multiple_of(PAGE_SIZE as u64);

        if offset == 0 {
            if expected_end <= self.end {
                self.start = expected_end;
                return Some(PageType::FullPage { number });
            } else {
                self.start = self.end;
                return Some(PageType::PartialPage {
                    number,
                    offset: 0,
                    size: (self.end - start) as usize,
                });
            }
        } else {
            if expected_end <= self.end {
                self.start = expected_end;
                return Some(PageType::PartialPage {
                    number,
                    offset: offset as usize,
                    size: (expected_end - start) as usize,
                });
            } else {
                self.start = self.end;
                return Some(PageType::PartialPage {
                    number,
                    offset: offset as usize,
                    size: (self.end - start) as usize,
                });
            }
        }
    }
}

/// A page in the cache.
pub struct CachePage {
    pub data: Box<[u8; PAGE_SIZE]>,
}

impl CachePage {
    pub fn new() -> Self {
        unsafe {
            Self {
                data: Box::new_zeroed().assume_init(),
            }
        }
    }
}

impl AsRef<[u8]> for CachePage {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}

impl AsMut<[u8]> for CachePage {
    fn as_mut(&mut self) -> &mut [u8] {
        self.data.as_mut()
    }
}

/// A backend using paged cache.
///
/// Will flush the dirty pages to the disk if and only if the `real_flush` is
/// called.
pub struct CachedBackend<B: Backend> {
    backend: B,
    cache: BTreeMap<u64, CachePage>,
    dirty: BTreeMap<u64, bool>,
    my_pos: u64, // seeking may also be very expensive
    my_len: u64, // we assume that the length of the file is fixed
}

impl<B: Backend> CachedBackend<B> {
    pub fn new(mut backend: B) -> Self {
        let my_len = backend.seek(SeekFrom::End(0)).unwrap();
        backend.seek(SeekFrom::Start(0)).unwrap();
        
        Self::new_with_len_known(backend, my_len)
    }

    pub fn new_with_len_known(backend: B, len: u64) -> Self {
        Self {
            backend,
            cache: BTreeMap::new(),
            dirty: BTreeMap::new(),
            my_pos: 0,
            my_len: len,
        }
    }
}

impl<B: Backend> Backend for CachedBackend<B> {
    fn open<P: AsRef<Path> + Send>(path: P) -> io::Result<Self> {
        B::open(path).map(Self::new)
    }

    fn create<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        let mut init_called = false;
        let mut backend = B::create(path, size, |_| {
            init_called = true;
            Ok(())
        })
        .map(|backend| Self::new_with_len_known(backend, size))?;

        if init_called {
            init(&mut backend)?;
        }
        Ok(backend)
    }

    fn create_new<P: AsRef<Path> + Send, F: FnOnce(&mut Self) -> io::Result<()>>(
        path: P,
        size: u64,
        init: F,
    ) -> io::Result<Self> {
        let mut init_called = false;
        let mut backend = B::create_new(path, size, |_| {
            init_called = true;
            Ok(())
        })
        .map(|backend| Self::new_with_len_known(backend, size))?;

        if init_called {
            init(&mut backend)?;
        }
        Ok(backend)
    }

    fn real_flush(&mut self) -> io::Result<()> {
        let origin_pos = self.backend.stream_position()?;

        for (offset, page) in self.cache.iter_mut() {
            if let Some(dirty) = self.dirty.get_mut(offset) {
                if *dirty {
                    self.backend.seek(SeekFrom::Start(*offset))?;
                    self.backend.write_all(page.data.as_ref())?;
                    *dirty = false;
                }
            }
        }

        self.backend.real_flush()?;
        self.backend.seek(SeekFrom::Start(origin_pos))?;

        Ok(())
    }
}

impl<B: Backend> io::Read for CachedBackend<B> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.my_pos;
        let mut read = 0;

        for page in PageRange::new(start, start + buf.len() as u64) {
            match page {
                PageType::FullPage { number } => {
                    if let Some(cache) = self.cache.get(&number) {
                        buf[read..read + PAGE_SIZE].copy_from_slice(cache.as_ref());
                    } else {
                        let mut cache = CachePage::new();
                        self.backend
                            .seek(SeekFrom::Start(number * PAGE_SIZE as u64))?;
                        self.backend.read_exact(cache.as_mut())?;
                        buf[read..read + PAGE_SIZE].copy_from_slice(cache.as_ref());
                        self.cache.insert(number, cache);
                        self.dirty.insert(number, false);
                    }
                    read += PAGE_SIZE;
                }
                PageType::PartialPage {
                    number,
                    offset,
                    size,
                } => {
                    if let Some(cache) = self.cache.get(&number) {
                        buf[read..read + size]
                            .copy_from_slice(&cache.data[offset..offset + size]);
                    } else {
                        let mut cache = CachePage::new();
                        self.backend
                            .seek(SeekFrom::Start(number * PAGE_SIZE as u64))?;
                        self.backend.read_exact(cache.as_mut())?;
                        buf[read..read + size]
                            .copy_from_slice(&cache.data[offset..offset + size]);
                        self.cache.insert(number, cache);
                        self.dirty.insert(number, false);
                    }
                    read += size;
                }
            }
        }

        self.my_pos += read as u64;
        Ok(read)
    }
}

impl<B: Backend> io::Write for CachedBackend<B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let start = self.my_pos;
        let mut written = 0;

        for page in PageRange::new(start, start + buf.len() as u64) {
            match page {
                PageType::FullPage { number } => {
                    if let Some(page) = self.cache.get_mut(&number) {
                        page.data
                            .copy_from_slice(&buf[written..written + PAGE_SIZE]);
                    } else {
                        let mut cache = CachePage::new();
                        cache.data[..].copy_from_slice(&buf[written..]);
                        self.cache.insert(number, cache);
                    }
                    self.dirty.insert(number, true);
                    written += PAGE_SIZE;
                }
                PageType::PartialPage {
                    number,
                    offset,
                    size,
                } => {
                    if let Some(page) = self.cache.get_mut(&number) {
                        page.data[offset..offset + size]
                            .copy_from_slice(&buf[written..written + size]);
                    } else {
                        let mut cache = CachePage::new();
                        {
                            let origin_pos = self.backend.stream_position()?;
                            self.backend
                                .seek(SeekFrom::Start(number * PAGE_SIZE as u64))?;
                            self.backend.read_exact(cache.data.as_mut())?;
                            self.backend.seek(SeekFrom::Start(origin_pos))?;
                        }
                        cache.data[offset..offset + size]
                            .copy_from_slice(&buf[written..written + size]);
                        self.cache.insert(number, cache);
                    }
                    self.dirty.insert(number, true);
                    written += size;
                }
            }
        }

        self.my_pos += written as u64;
        Ok(written)
    }

    /// We don't need to flush the cache here
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<B: Backend> io::Seek for CachedBackend<B> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.my_pos = match pos {
            SeekFrom::Start(pos) => pos,
            SeekFrom::End(pos) => self.my_len.wrapping_add_signed(pos),
            SeekFrom::Current(pos) => self.my_pos.wrapping_add_signed(pos),
        };
        Ok(self.my_pos)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.my_pos)
    }
}

impl<B: Backend> Drop for CachedBackend<B> {
    fn drop(&mut self) {
        self.real_flush().unwrap();
    }
}
