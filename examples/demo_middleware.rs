#![feature(allocator_api)]

use std::{
    alloc::Global,
    hint::black_box,
    io::{Read, Write},
    iter,
    time::Instant,
};

use futures_lite::future::yield_now;
use spin_on::spin_on;
use time::{ext::InstantExt, Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use unico::asym::{sync, AsymWait};
use unico_context::{boost::Boost, global_resumer};
use unico_stack::global_stack_allocator;

global_resumer!(Boost);
global_stack_allocator!(Global);

/// A simple and synchronous middleware that accepts only synchronous callback
/// functions.
fn demo_middleware<I, O>(mut input_fn: I, mut output_fn: O) -> Result<u8, ()>
where
    I: FnMut(&mut [u8]) -> Result<usize, ()>,
    O: FnMut(&[u8]) -> Result<(), ()>,
{
    const BLOCK_SIZE: usize = 1024;

    let mut buf = vec![0; BLOCK_SIZE];
    let mut result = 0;

    loop {
        let read = input_fn(&mut buf)?;
        if read == 0 {
            return Ok(result);
        }

        // some processing here, we use prefix xor as an example
        //
        // this middleware implements in-block prefix xor and returns the final xor
        // result
        for i in 1..read {
            buf[i] ^= buf[i - 1];
        }

        result ^= buf[read - 1];

        // there may be more callbacks here, other than the output_fn
        output_fn(&buf[..read])?;
    }
}

fn main() {
    let input_file = "LICENSE-APACHE";
    // 1. full sync
    {
        let mut input_file = std::fs::File::open(input_file).unwrap();
        let mut output_file = std::fs::File::create("./target/LICENSE-APACHE-demo-sync").unwrap();
        let input_fn = |buf: &mut [u8]| input_file.read(buf).map_err(|_| ());
        let output_fn = |buf: &[u8]| output_file.write_all(buf).map_err(|_| ());
        let result = demo_middleware(input_fn, output_fn).unwrap();
        println!("sync result: {}", result);
    }

    // 2. async with out unico, i.e.
    // a. use sync read and write, or
    // b. manually poll the async read and write
    //
    // maybe not implementable in tokio? not sure if tokio allow two runtimes in the
    // same time

    // 3. async with unico
    {
        let f = async {
            // async file
            let mut input_file = tokio::fs::File::open(input_file).await.unwrap();
            let mut output_file = tokio::fs::File::create("./target/LICENSE-APACHE-demo-unico")
                .await
                .unwrap();
            // async read and write wrapped by unico
            let input_fn = |buf: &mut [u8]| {
                AsyncReadExt::read(&mut input_file, buf)
                    .wait()
                    .map_err(|_| ())
            };
            let output_fn =
                |buf: &[u8]| output_file.write_all(buf).wait().map_err(|_| ());
            sync(|| demo_middleware(input_fn, output_fn).unwrap()).await
        };

        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f);

        println!("unico result: {}", result);
    }
}
