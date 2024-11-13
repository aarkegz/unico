use time::Duration;

pub struct TestResult {
    // The duration of the tested block
    pub duration: Duration,
    // The duration of the baseline block
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

#[macro_export]
macro_rules! bench_with_times {
    ($times:ident => $tested_block:block - $baseline_block:block) => {
        {
            use time::ext::InstantExt;

            let start = std::time::Instant::now();
            $tested_block
            let duration = std::time::Instant::now().signed_duration_since(start) / $times;

            let start = std::time::Instant::now();
            $baseline_block
            let baseline = std::time::Instant::now().signed_duration_since(start) / $times;

            $crate::TestResult { duration, baseline }
        }
    };
}

#[macro_export]
macro_rules! bench_matrix {
    ($desc:literal: $total_run:expr, $times:ident => $tested_block:block - $baseline_block:block) => {
        {
            let mut repeat = 1u32;
            let mut diffs = vec![];
            while repeat <= $total_run {
                let times_per_repeat = $total_run / repeat;
                let $times = times_per_repeat;

                let result = std::iter::repeat_with(|| $crate::bench_with_times!($times => $tested_block - $baseline_block))
                    .take(repeat as usize)
                    .sum::<$crate::TestResult>();

                let duration = result.duration / repeat;
                let baseline = result.baseline / repeat;
                let diff = duration - baseline;

                println!("{}: times = {}, repeat = {}: {}, with baseline {}, diff {}", $desc, times_per_repeat, repeat, duration, baseline, diff);

                diffs.push(diff);
                repeat *= 2;
            }

            let avg = diffs.iter().sum::<time::Duration>() / diffs.len() as u32;
            println!("{}: avg diff {}", $desc, avg);
        }
    };
}
