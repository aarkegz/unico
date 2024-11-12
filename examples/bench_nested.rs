#![feature(allocator_api)]

use std::{alloc::Global, hint::black_box, io::Read, iter, time::Instant};

use futures_lite::{AsyncRead, AsyncReadExt};
use spin_on::spin_on;
use time::{ext::InstantExt, Duration};
use unico::asym::{sync, AsymWait};
use unico_context::{boost::Boost, global_resumer};
use unico_stack::global_stack_allocator;

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

struct TestResult {
    pub duration: Duration,
    pub baseline: Duration,
}

impl std::iter::Sum for TestResult {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(
            Self {
                duration: Duration::ZERO,
                baseline: Duration::ZERO,
            },
            |acc, r| Self {
                duration: acc.duration + r.duration,
                baseline: acc.baseline + r.baseline,
            },
        )
    }
}

#[inline(never)]
fn test(times: u32) -> TestResult {
    const SIZE: usize = 600;

    let start = Instant::now();
    spin_on(black_box(async {
        for _ in 0..times {
            let r: &[u8] = &[0x12; SIZE];
            let mut buf = [0u8; SIZE];
            read_synced(&mut { r }, &mut buf).await;
        }
    }));
    let synced = Instant::now().signed_duration_since(start) / times;

    let start = Instant::now();
    spin_on(black_box(async {
        for _ in 0..times {
            let r: &[u8] = &[0x12; SIZE];
            let mut buf = [0u8; SIZE];
            read_direct(&mut { r }, &mut buf).await;
        }
    }));
    let direct = Instant::now().signed_duration_since(start) / times;

    TestResult {
        duration: synced,
        baseline: direct,
    }
}

fn main() {
    const NUMS: &[u32] = &[
        1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288, 1048576,
    ];

    let sum = NUMS.iter().fold(Duration::ZERO, |acc, &num| {
        let repeat = 1048576 / num;

        let total = iter::repeat_with(|| test(num))
            .take(repeat as usize)
            .sum::<TestResult>();

        let duration = total.duration / repeat;
        let baseline = total.baseline / repeat;
        let diff = duration - baseline;

        println!(
            "nested {} times: {}, duration: {}, baseline: {}",
            num, diff, duration, baseline
        );
        acc + diff
    });

    println!("avr: {}", sum / NUMS.len() as u32);
}
