//! Bounded buffers of time-stamped samples for live line charts.
//!
//! Each channel owns its own [`Ring`]. A ring holds at most `capacity` samples,
//! and the oldest one is dropped when a new one arrives. Timestamps are seconds
//! on any monotonic clock. They must be finite and must never decrease, so every
//! window query can use binary search.

use std::collections::VecDeque;

/// One time-stamped reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Seconds on the caller's clock.
    pub t: f64,
    pub value: f32,
}

/// Channels that can be charted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Throttle,
    Brake,
    Steering,
    Speed,
    Gear,
}

impl Channel {
    /// Every channel, in display order.
    pub const ALL: [Channel; 5] = [
        Channel::Throttle,
        Channel::Brake,
        Channel::Steering,
        Channel::Speed,
        Channel::Gear,
    ];

    /// Human-readable name for legends and headings.
    pub fn label(self) -> &'static str {
        match self {
            Channel::Throttle => "Throttle",
            Channel::Brake => "Brake",
            Channel::Steering => "Steering",
            Channel::Speed => "Speed",
            Channel::Gear => "Gear",
        }
    }

    fn index(self) -> usize {
        match self {
            Channel::Throttle => 0,
            Channel::Brake => 1,
            Channel::Steering => 2,
            Channel::Speed => 3,
            Channel::Gear => 4,
        }
    }
}

/// A fixed-capacity buffer of samples, oldest first.
#[derive(Debug, Clone)]
pub struct Ring {
    capacity: usize,
    samples: VecDeque<Sample>,
}

impl Ring {
    /// Creates an empty ring. A capacity of zero is raised to one.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The newest sample, if any.
    pub fn last(&self) -> Option<Sample> {
        self.samples.back().copied()
    }

    /// Drops every sample.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Appends a sample, evicting the oldest one when the ring is full.
    ///
    /// Returns `false` and stores nothing when `t` or `value` is not finite, or
    /// when `t` is earlier than the newest stored sample.
    pub fn push(&mut self, t: f64, value: f32) -> bool {
        if !t.is_finite() || !value.is_finite() {
            return false;
        }
        if self.samples.back().is_some_and(|last| t < last.t) {
            return false;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample { t, value });
        true
    }

    /// Index range of the samples with `from <= t <= to`. Empty when the bounds
    /// are inverted or NaN.
    fn range_of(&self, from: f64, to: f64) -> std::ops::Range<usize> {
        if !(from <= to) {
            return 0..0;
        }
        let start = self.samples.partition_point(|s| s.t < from);
        let end = self.samples.partition_point(|s| s.t <= to);
        start..end.max(start)
    }

    /// Samples with `from <= t <= to`, oldest first.
    pub fn window(&self, from: f64, to: f64) -> Vec<Sample> {
        self.samples.range(self.range_of(from, to)).copied().collect()
    }

    /// Smallest and largest value with `from <= t <= to`, or `None` when that
    /// window is empty.
    pub fn min_max(&self, from: f64, to: f64) -> Option<(f32, f32)> {
        self.samples
            .range(self.range_of(from, to))
            .map(|s| s.value)
            .fold(None, |acc, v| match acc {
                None => Some((v, v)),
                Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
            })
    }

    /// Reduces the samples in `from..=to` to at most `points` samples.
    ///
    /// When the window already holds `points` samples or fewer, they are all
    /// returned unchanged. Otherwise the window is split into `points` runs of
    /// nearly equal length, and each run becomes one sample holding the mean time
    /// and mean value of its members. Returns an empty vector when `points` is 0.
    pub fn downsample(&self, from: f64, to: f64, points: usize) -> Vec<Sample> {
        if points == 0 {
            return Vec::new();
        }
        let range = self.range_of(from, to);
        let n = range.len();
        let window = self.samples.range(range);
        if n <= points {
            return window.copied().collect();
        }
        let window: Vec<Sample> = window.copied().collect();
        (0..points)
            .map(|i| {
                let lo = i * n / points;
                let hi = (i + 1) * n / points;
                let run = &window[lo..hi];
                let count = run.len() as f64;
                let t = run.iter().map(|s| s.t).sum::<f64>() / count;
                let value = run.iter().map(|s| f64::from(s.value)).sum::<f64>() / count;
                Sample {
                    t,
                    value: value as f32,
                }
            })
            .collect()
    }
}

/// One [`Ring`] per [`Channel`], all with the same capacity.
#[derive(Debug, Clone)]
pub struct ChannelBuffers {
    rings: [Ring; 5],
}

impl ChannelBuffers {
    pub fn new(capacity: usize) -> Self {
        Self {
            rings: std::array::from_fn(|_| Ring::new(capacity)),
        }
    }

    /// Pushes one reading onto a channel. Same rules as [`Ring::push`].
    pub fn push(&mut self, channel: Channel, t: f64, value: f32) -> bool {
        self.rings[channel.index()].push(t, value)
    }

    pub fn ring(&self, channel: Channel) -> &Ring {
        &self.rings[channel.index()]
    }

    /// Empties every channel.
    pub fn clear(&mut self) {
        for ring in &mut self.rings {
            ring.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(values: &[(f64, f32)]) -> Ring {
        let mut ring = Ring::new(64);
        for &(t, v) in values {
            assert!(ring.push(t, v));
        }
        ring
    }

    #[test]
    fn zero_capacity_is_raised_to_one() {
        let mut ring = Ring::new(0);
        assert_eq!(ring.capacity(), 1);
        assert!(ring.push(0.0, 1.0));
        assert!(ring.push(1.0, 2.0));
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.last().unwrap().value, 2.0);
    }

    #[test]
    fn push_rejects_non_finite_and_backwards_time() {
        let mut ring = Ring::new(8);
        assert!(!ring.push(f64::NAN, 1.0));
        assert!(!ring.push(0.0, f32::INFINITY));
        assert!(!ring.push(f64::NEG_INFINITY, 1.0));
        assert!(ring.push(5.0, 1.0));
        assert!(!ring.push(4.9, 1.0));
        assert!(ring.push(5.0, 2.0), "equal timestamps are allowed");
        assert_eq!(ring.len(), 2);
    }

    #[test]
    fn full_ring_drops_the_oldest_sample() {
        let mut ring = Ring::new(3);
        for i in 0..5 {
            assert!(ring.push(i as f64, i as f32));
        }
        assert_eq!(ring.len(), 3);
        let values: Vec<f32> = ring.window(f64::MIN, f64::MAX).iter().map(|s| s.value).collect();
        assert_eq!(values, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn window_is_inclusive_and_empty_for_inverted_bounds() {
        let ring = filled(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]);
        let ts: Vec<f64> = ring.window(1.0, 2.0).iter().map(|s| s.t).collect();
        assert_eq!(ts, vec![1.0, 2.0]);
        assert!(ring.window(2.5, 2.4).is_empty());
        assert!(ring.window(f64::NAN, 2.0).is_empty());
        assert!(ring.window(10.0, 20.0).is_empty());
    }

    #[test]
    fn min_max_covers_only_the_window() {
        let ring = filled(&[(0.0, 9.0), (1.0, -2.0), (2.0, 4.0), (3.0, 100.0)]);
        assert_eq!(ring.min_max(1.0, 2.0), Some((-2.0, 4.0)));
        assert_eq!(ring.min_max(0.0, 3.0), Some((-2.0, 100.0)));
        assert_eq!(ring.min_max(50.0, 60.0), None);
        assert_eq!(Ring::new(4).min_max(0.0, 1.0), None);
    }

    #[test]
    fn downsample_returns_short_windows_unchanged() {
        let ring = filled(&[(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)]);
        let out = ring.downsample(0.0, 2.0, 5);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1], Sample { t: 1.0, value: 2.0 });
    }

    #[test]
    fn downsample_produces_exactly_n_bucket_means() {
        let values: Vec<(f64, f32)> = (0..10).map(|i| (i as f64, i as f32)).collect();
        let ring = filled(&values);
        let out = ring.downsample(0.0, 9.0, 2);
        assert_eq!(out.len(), 2);
        // Buckets are indices 0..5 and 5..10.
        assert!((out[0].t - 2.0).abs() < 1e-9);
        assert!((out[0].value - 2.0).abs() < 1e-6);
        assert!((out[1].t - 7.0).abs() < 1e-9);
        assert!((out[1].value - 7.0).abs() < 1e-6);
    }

    #[test]
    fn downsample_zero_points_is_empty() {
        let ring = filled(&[(0.0, 1.0), (1.0, 2.0)]);
        assert!(ring.downsample(0.0, 1.0, 0).is_empty());
    }

    #[test]
    fn channel_buffers_keep_channels_separate() {
        let mut bufs = ChannelBuffers::new(16);
        assert!(bufs.push(Channel::Speed, 0.0, 120.0));
        assert!(bufs.push(Channel::Gear, 0.0, 4.0));
        assert_eq!(bufs.ring(Channel::Speed).len(), 1);
        assert_eq!(bufs.ring(Channel::Gear).len(), 1);
        assert!(bufs.ring(Channel::Throttle).is_empty());
        assert_eq!(bufs.ring(Channel::Speed).last().unwrap().value, 120.0);
    }

    #[test]
    fn channel_buffers_clear_empties_every_channel() {
        let mut bufs = ChannelBuffers::new(16);
        for channel in Channel::ALL {
            assert!(bufs.push(channel, 1.0, 0.5));
        }
        bufs.clear();
        for channel in Channel::ALL {
            assert!(bufs.ring(channel).is_empty(), "{}", channel.label());
        }
    }

    #[test]
    fn channel_buffers_reject_out_of_order_per_channel() {
        let mut bufs = ChannelBuffers::new(16);
        assert!(bufs.push(Channel::Brake, 2.0, 1.0));
        assert!(!bufs.push(Channel::Brake, 1.0, 1.0));
        assert!(bufs.push(Channel::Steering, 1.0, 1.0), "other channels have their own clock order");
    }
}
