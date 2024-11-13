#![feature(allocator_api)]

use std::{alloc::Global, hint::black_box};

use bencher::bench_matrix;
use futures_lite::future::yield_now;
use spin_on::spin_on;
use unico::{
    asym::{sync, AsymWait},
    context::{boost::Boost, global_resumer},
    stack::global_stack_allocator,
};

global_resumer!(Boost);
global_stack_allocator!(Global);

fn main() {
    bench_matrix!("yield": 1048576, times => {
        spin_on(black_box(async {
            sync(|| {
                for _ in 0..times {
                    yield_now().wait();
                }
            })
            .await;
        }));
    } - {
        spin_on(black_box(async {
            for _ in 0..times {
                yield_now().await;
            }
        }));
    });
}
