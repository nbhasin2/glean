// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Bucketing, Histogram};

// The following are defaults for a simple timing distribution for the default time unit
// of millisecond.  The values arrived at were approximated using existing "_MS"
// telemetry probes as a guide.
const DEFAULT_BUCKET_COUNT: usize = 100;
const DEFAULT_RANGE_MIN: u64 = 0;
const DEFAULT_RANGE_MAX: u64 = 60_000;

/// Create the possible ranges in an exponential distribution from `min` to `max` with
/// `bucket_count` buckets.
///
/// This algorithm calculates the bucket sizes using a natural log approach to get `bucket_count` number of buckets,
/// exponentially spaced between `min` and `max`
///
/// Bucket limits are the minimal bucket value.
/// That means values in a bucket `i` are `bucket[i] <= value < bucket[i+1]`.
/// It will always contain an underflow bucket (`< 1`).
fn exponential_range(min: u64, max: u64, bucket_count: usize) -> Vec<u64> {
    let log_max = (max as f64).ln();

    let mut ranges = Vec::with_capacity(bucket_count);
    let mut current = min;
    if current == 0 {
        current = 1;
    }

    // undeflow bucket
    ranges.push(0);
    ranges.push(current);

    for i in 2..bucket_count {
        let log_current = (current as f64).ln();
        let log_ratio = (log_max - log_current) / (bucket_count - i) as f64;
        let log_next = log_current + log_ratio;
        let next_value = log_next.exp().round() as u64;
        current = if next_value > current {
            next_value
        } else {
            current + 1
        };
        ranges.push(current);
    }

    ranges
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exponential {
    bucket_ranges: Vec<u64>,
}

impl Bucketing for Exponential {
    /// Get the bucket for the sample.
    ///
    /// This uses a binary search to locate the index `i` of the bucket such that:
    /// bucket[i] <= sample < bucket[i+1]
    fn sample_to_bucket_minimum(&self, sample: u64) -> u64 {
        let limit = match self.bucket_ranges.binary_search(&sample) {
            // Found an exact match to fit it in
            Ok(i) => i,
            // Sorted it fits after the bucket's limit, therefore it fits into the previous bucket
            Err(i) => i - 1,
        };

        self.bucket_ranges[limit]
    }
}

impl Default for Histogram<Exponential> {
    fn default() -> Histogram<Exponential> {
        Histogram::exponential(DEFAULT_RANGE_MIN, DEFAULT_RANGE_MAX, DEFAULT_BUCKET_COUNT)
    }
}

impl Histogram<Exponential> {
    /// Create a histogram with `count` exponential buckets in the range `min` to `max`.
    pub fn exponential(min: u64, max: u64, bucket_count: usize) -> Histogram<Exponential> {
        let bucket_ranges = exponential_range(min, max, bucket_count);

        Histogram {
            values: HashMap::new(),
            count: 0,
            sum: 0,
            bucketing: Exponential { bucket_ranges },
        }
    }

    /// Get a snapshot of _all_ values.
    pub fn snapshot_values(&self) -> HashMap<u64, u64> {
        let mut res = self.values.clone();

        for &min_bucket in &self.bucketing.bucket_ranges {
            // Fill in missing entries.
            let _ = res.entry(min_bucket).or_insert(0);
        }
        res
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_count() {
        let mut hist = Histogram::exponential(1, 500, 10);
        assert!(hist.is_empty());

        for i in 1..=10 {
            hist.accumulate(i);
        }

        assert_eq!(10, hist.count());
        assert_eq!(55, hist.sum());
    }

    #[test]
    fn overflow_values_accumulate_in_the_last_bucket() {
        let mut hist = Histogram::default();

        hist.accumulate(DEFAULT_RANGE_MAX + 100);
        assert_eq!(1, hist.values[&DEFAULT_RANGE_MAX]);
    }

    #[test]
    fn short_exponential_buckets_are_correct() {
        let test_buckets = vec![0, 1, 2, 3, 5, 9, 16, 29, 54, 100];

        assert_eq!(test_buckets, exponential_range(1, 100, 10));
        // There's always a zero bucket, so we increase the lower limit.
        assert_eq!(test_buckets, exponential_range(0, 100, 10));
    }

    #[test]
    fn default_exponential_buckets_are_correct() {
        // Hand calculated values using current default range 0 - 60000 and bucket count of 100.
        // NOTE: The final bucket, regardless of width, represents the overflow bucket to hold any
        // values beyond the maximum (in this case the maximum is 60000)
        let test_buckets = vec![
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 19, 21, 23, 25, 28, 31, 34,
            38, 42, 46, 51, 56, 62, 68, 75, 83, 92, 101, 111, 122, 135, 149, 164, 181, 200, 221,
            244, 269, 297, 328, 362, 399, 440, 485, 535, 590, 651, 718, 792, 874, 964, 1064, 1174,
            1295, 1429, 1577, 1740, 1920, 2118, 2337, 2579, 2846, 3140, 3464, 3822, 4217, 4653,
            5134, 5665, 6250, 6896, 7609, 8395, 9262, 10219, 11275, 12440, 13726, 15144, 16709,
            18436, 20341, 22443, 24762, 27321, 30144, 33259, 36696, 40488, 44672, 49288, 54381,
            60000,
        ];

        assert_eq!(
            test_buckets,
            exponential_range(DEFAULT_RANGE_MIN, DEFAULT_RANGE_MAX, DEFAULT_BUCKET_COUNT)
        );
    }

    #[test]
    fn default_buckets_correctly_accumulate() {
        let mut hist = Histogram::default();

        for i in &[1, 10, 100, 1000, 10000] {
            hist.accumulate(*i);
        }

        assert_eq!(11111, hist.sum());
        assert_eq!(5, hist.count());

        assert_eq!(None, hist.values.get(&0)); // underflow is empty
        assert_eq!(1, hist.values[&1]); // bucket_ranges[1]  = 1
        assert_eq!(1, hist.values[&10]); // bucket_ranges[10] = 10
        assert_eq!(1, hist.values[&92]); // bucket_ranges[33] = 92
        assert_eq!(1, hist.values[&964]); // bucket_ranges[57] = 964
        assert_eq!(1, hist.values[&9262]); // bucket_ranges[80] = 9262
    }

    #[test]
    fn accumulate_large_numbers() {
        let mut hist = Histogram::exponential(1, 500, 10);

        hist.accumulate(u64::max_value());
        hist.accumulate(u64::max_value());

        assert_eq!(2, hist.count());
        // Saturate before overflowing
        assert_eq!(u64::max_value(), hist.sum());
        assert_eq!(2, hist.values[&500]);
    }
}
