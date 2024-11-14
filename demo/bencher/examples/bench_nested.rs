#![feature(allocator_api)]

use std::{alloc::Global, hint::black_box, io::Read};

use bencher::bench_matrix;
use futures_lite::{AsyncRead, AsyncReadExt};
use spin_on::spin_on;
use unico::{
    asym::{sync, AsymWait},
    context::{boost::Boost, global_resumer},
    stack::global_stack_allocator,
};

global_resumer!(Boost);
global_stack_allocator!(Global);

struct Synced<R>(R);

impl<R: AsyncRead + Unpin + Send> Read for Synced<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf).wait()
    }
}

async fn read_synced(
    r: &mut (impl AsyncRead + Unpin + Send),
    buf: &mut [u8],
) -> std::io::Result<usize> {
    sync(|| Synced(r).read(buf)).await
}

async fn read_direct(
    r: &mut (impl AsyncRead + Unpin + Send),
    buf: &mut [u8],
) -> std::io::Result<usize> {
    r.read(buf).await
}

fn main() {
    const SIZE: usize = 600;

    bench_matrix!("nested": 1048576, times => {
        spin_on(black_box(async {
            for _ in 0..times {
                let r: &[u8] = &[0x12; SIZE];
                let mut buf = [0u8; SIZE];
                read_synced(&mut { r }, &mut buf).await.unwrap();
            }
        }));
    } - {
        spin_on(black_box(async {
            for _ in 0..times {
                let r: &[u8] = &[0x12; SIZE];
                let mut buf = [0u8; SIZE];
                read_direct(&mut { r }, &mut buf).await.unwrap();
            }
        }));
    });
}
