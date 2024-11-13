#![feature(allocator_api)]

use std::{alloc::Global, hint::black_box};

use bencher::bench_matrix;
use spin_on::spin_on;
use unico::{
    asym::sync,
    context::{boost::Boost, global_resumer},
    stack::global_stack_allocator,
};

global_resumer!(Boost);
global_stack_allocator!(Global);

fn main() {
    bench_matrix!("create": 1048576, times => {
        for _ in 0..times {
            spin_on(black_box(async {
                sync(|| {}).await;
            }));
        }
    } - {
        for _ in 0..times {
            spin_on(black_box(async {}));
        }
    });
}
