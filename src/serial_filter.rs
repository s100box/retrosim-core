// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Serial filter trait and pipeline for transport data transformation.
//!
//! Filters sit between a UART card port and a host transport, transforming
//! bytes flowing in either direction. They are stateful (may buffer partial
//! escape sequences) and applied in pipeline order for TX (guest→host) and
//! reverse order for RX (host→guest).

use std::borrow::Cow;

/// A bidirectional byte-stream filter for serial port data.
///
/// Filters transform bytes flowing between a UART card port and a host
/// transport. They are stateful: a filter may buffer partial escape
/// sequences, track cursor position, or maintain encoding tables.
///
/// Filters are applied in pipeline order for TX (guest→host) and in
/// reverse order for RX (host→guest).
pub trait SerialFilter: Send {
    /// Filter bytes in the TX direction (guest → host).
    ///
    /// Consumes `input` and returns transformed bytes. May return
    /// fewer or more bytes than the input (e.g., expanding a single
    /// control code into a multi-byte escape sequence). May return
    /// an empty `Vec` if buffering an incomplete sequence.
    fn filter_tx(&mut self, input: &[u8]) -> Vec<u8>;

    /// Filter bytes in the RX direction (host → guest).
    ///
    /// Same semantics as [`filter_tx`](Self::filter_tx) but for the
    /// reverse direction.
    fn filter_rx(&mut self, input: &[u8]) -> Vec<u8>;

    /// Human-readable name of this filter (e.g., `"7bit"`, `"tvi925-to-ansi"`).
    fn name(&self) -> &str;

    /// Reset filter state (e.g., on transport reconnect or UART reset).
    fn reset(&mut self);

    /// Drain any pending loopback data buffered by this filter.
    ///
    /// The default implementation returns `None` (no loopback).
    /// A loopback filter captures TX data here so the transport
    /// manager can inject it back as RX input.
    fn pending_loopback(&mut self) -> Option<Vec<u8>> {
        None
    }
}

/// An ordered chain of [`SerialFilter`] instances for one port.
///
/// Filters are applied in list order for TX (guest→host) and in reverse
/// order for RX (host→guest).
pub struct FilterPipeline {
    filters: Vec<Box<dyn SerialFilter>>,
}

impl FilterPipeline {
    /// Create an empty pipeline (pass-through).
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Append a filter to the end of the pipeline.
    pub fn push(&mut self, filter: Box<dyn SerialFilter>) {
        self.filters.push(filter);
    }

    /// Insert a filter at a specific position.
    ///
    /// # Panics
    ///
    /// Panics if `index > self.len()`.
    pub fn insert(&mut self, index: usize, filter: Box<dyn SerialFilter>) {
        self.filters.insert(index, filter);
    }

    /// Remove the filter at `index`. Returns the removed filter's name.
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    pub fn remove(&mut self, index: usize) -> String {
        let f = self.filters.remove(index);
        f.name().to_string()
    }

    /// List the filter names in pipeline order.
    pub fn list(&self) -> Vec<&str> {
        self.filters.iter().map(|f| f.name()).collect()
    }

    /// Number of filters in the pipeline.
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Returns true if the pipeline has no filters.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Process TX bytes through all filters (first → last).
    ///
    /// Returns `Cow::Borrowed` when the pipeline is empty (zero allocation).
    pub fn apply_tx<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        let mut buf = Cow::Borrowed(data);
        for f in &mut self.filters {
            buf = Cow::Owned(f.filter_tx(&buf));
            if buf.is_empty() {
                break;
            }
        }
        buf
    }

    /// Process RX bytes through all filters (last → first).
    ///
    /// Returns `Cow::Borrowed` when the pipeline is empty (zero allocation).
    pub fn apply_rx<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        let mut buf = Cow::Borrowed(data);
        for f in self.filters.iter_mut().rev() {
            buf = Cow::Owned(f.filter_rx(&buf));
            if buf.is_empty() {
                break;
            }
        }
        buf
    }

    /// Reset all filters in the pipeline.
    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f.reset();
        }
    }

    /// Returns `true` if any filter is named `"loopback"`.
    pub fn has_loopback(&self) -> bool {
        self.filters.iter().any(|f| f.name() == "loopback")
    }

    /// Drain pending loopback data from all filters.
    ///
    /// Returns accumulated bytes that should be injected back as RX input.
    pub fn drain_loopback(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for f in &mut self.filters {
            if let Some(data) = f.pending_loopback() {
                out.extend_from_slice(&data);
            }
        }
        out
    }
}

impl Default for FilterPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test filter that adds a fixed offset to each byte.
    struct OffsetFilter {
        tx_offset: u8,
        rx_offset: u8,
        name: &'static str,
    }

    impl SerialFilter for OffsetFilter {
        fn filter_tx(&mut self, input: &[u8]) -> Vec<u8> {
            input
                .iter()
                .map(|&b| b.wrapping_add(self.tx_offset))
                .collect()
        }
        fn filter_rx(&mut self, input: &[u8]) -> Vec<u8> {
            input
                .iter()
                .map(|&b| b.wrapping_add(self.rx_offset))
                .collect()
        }
        fn name(&self) -> &str {
            self.name
        }
        fn reset(&mut self) {}
    }

    /// A filter that drops all bytes (returns empty).
    struct DropFilter;

    impl SerialFilter for DropFilter {
        fn filter_tx(&mut self, _input: &[u8]) -> Vec<u8> {
            Vec::new()
        }
        fn filter_rx(&mut self, _input: &[u8]) -> Vec<u8> {
            Vec::new()
        }
        fn name(&self) -> &str {
            "drop"
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn empty_pipeline_is_passthrough() {
        let mut p = FilterPipeline::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
        assert_eq!(&*p.apply_tx(b"hello"), b"hello");
        assert_eq!(&*p.apply_rx(b"world"), b"world");
    }

    #[test]
    fn empty_pipeline_returns_borrowed() {
        let mut p = FilterPipeline::new();
        assert!(matches!(p.apply_tx(b"data"), Cow::Borrowed(_)));
        assert!(matches!(p.apply_rx(b"data"), Cow::Borrowed(_)));
    }

    #[test]
    fn non_empty_pipeline_returns_owned() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "identity",
        }));
        assert!(matches!(p.apply_tx(b"data"), Cow::Owned(_)));
        assert!(matches!(p.apply_rx(b"data"), Cow::Owned(_)));
    }

    #[test]
    fn single_filter_tx_and_rx() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 1,
            rx_offset: 2,
            name: "off1",
        }));
        assert_eq!(&*p.apply_tx(&[0, 1, 2]), &[1, 2, 3]);
        assert_eq!(&*p.apply_rx(&[0, 1, 2]), &[2, 3, 4]);
    }

    #[test]
    fn tx_applies_forward_rx_applies_reverse() {
        struct DoubleFilter;

        impl SerialFilter for DoubleFilter {
            fn filter_tx(&mut self, input: &[u8]) -> Vec<u8> {
                input.iter().map(|byte| byte.wrapping_mul(2)).collect()
            }
            fn filter_rx(&mut self, input: &[u8]) -> Vec<u8> {
                self.filter_tx(input)
            }
            fn name(&self) -> &str {
                "double"
            }
            fn reset(&mut self) {}
        }

        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 1,
            rx_offset: 10,
            name: "A",
        }));
        p.push(Box::new(DoubleFilter));

        // These operations do not commute, so swapping their order fails.
        // TX: (1 + 1) * 2 = 4.
        assert_eq!(&*p.apply_tx(&[1]), &[4]);

        // RX: (1 * 2) + 10 = 12.
        assert_eq!(&*p.apply_rx(&[1]), &[12]);
    }

    #[test]
    fn empty_output_short_circuits() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(DropFilter));
        p.push(Box::new(OffsetFilter {
            tx_offset: 1,
            rx_offset: 1,
            name: "never-reached",
        }));
        assert!(p.apply_tx(b"data").is_empty());

        // For RX, reverse order: OffsetFilter runs first, then DropFilter
        assert!(p.apply_rx(b"data").is_empty());

        // Exercise name()/reset() on both filter types.
        assert!(!p.has_loopback());
        p.reset();
    }

    #[test]
    fn list_names() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "alpha",
        }));
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "beta",
        }));
        assert_eq!(p.list(), vec!["alpha", "beta"]);
    }

    #[test]
    fn remove_by_index() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "alpha",
        }));
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "beta",
        }));
        let removed = p.remove(0);
        assert_eq!(removed, "alpha");
        assert_eq!(p.list(), vec!["beta"]);
    }

    #[test]
    fn insert_at_position() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "first",
        }));
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "third",
        }));
        p.insert(
            1,
            Box::new(OffsetFilter {
                tx_offset: 0,
                rx_offset: 0,
                name: "second",
            }),
        );
        assert_eq!(p.list(), vec!["first", "second", "third"]);
    }

    #[test]
    fn reset_propagates() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        struct ResetTracker {
            resets: Arc<AtomicU32>,
        }
        impl SerialFilter for ResetTracker {
            fn filter_tx(&mut self, input: &[u8]) -> Vec<u8> {
                input.to_vec()
            }
            fn filter_rx(&mut self, input: &[u8]) -> Vec<u8> {
                input.to_vec()
            }
            fn name(&self) -> &str {
                "tracker"
            }
            fn reset(&mut self) {
                self.resets.fetch_add(1, Ordering::Relaxed);
            }
        }

        let count = Arc::new(AtomicU32::new(0));
        let mut p = FilterPipeline::new();
        p.push(Box::new(ResetTracker {
            resets: count.clone(),
        }));
        p.push(Box::new(ResetTracker {
            resets: count.clone(),
        }));
        // Exercise the filter_tx/filter_rx/name paths as well.
        assert_eq!(&*p.apply_tx(b"x"), b"x");
        assert_eq!(&*p.apply_rx(b"y"), b"y");
        assert_eq!(p.list(), vec!["tracker", "tracker"]);
        p.reset();
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn loopback_detection_and_drain() {
        /// A loopback filter: captures TX bytes and offers them back as
        /// pending loopback data.
        struct Loopback {
            buf: Vec<u8>,
        }
        impl SerialFilter for Loopback {
            fn filter_tx(&mut self, input: &[u8]) -> Vec<u8> {
                self.buf.extend_from_slice(input);
                input.to_vec()
            }
            fn filter_rx(&mut self, input: &[u8]) -> Vec<u8> {
                input.to_vec()
            }
            fn name(&self) -> &str {
                "loopback"
            }
            fn reset(&mut self) {
                self.buf.clear();
            }
            fn pending_loopback(&mut self) -> Option<Vec<u8>> {
                if self.buf.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut self.buf))
                }
            }
        }

        let mut p = FilterPipeline::new();
        // A normal filter that keeps the default pending_loopback() (None).
        p.push(Box::new(OffsetFilter {
            tx_offset: 0,
            rx_offset: 0,
            name: "passthru",
        }));
        assert!(!p.has_loopback());
        // No loopback filter present: drain hits the default None path.
        assert!(p.drain_loopback().is_empty());

        p.push(Box::new(Loopback { buf: Vec::new() }));
        assert!(p.has_loopback());

        // RX flows through both filters unchanged.
        assert_eq!(&*p.apply_rx(b"in"), b"in");

        // TX flows through; the loopback filter captures it.
        let _ = p.apply_tx(b"echo");
        assert_eq!(p.drain_loopback(), b"echo");
        // Buffer consumed: a second drain is empty.
        assert!(p.drain_loopback().is_empty());

        // reset() clears loopback state and propagates to every filter.
        let _ = p.apply_tx(b"more");
        p.reset();
        assert!(p.drain_loopback().is_empty());
    }

    #[test]
    fn default_is_empty() {
        let p = FilterPipeline::default();
        assert!(p.is_empty());
    }

    #[test]
    fn apply_to_empty_input() {
        let mut p = FilterPipeline::new();
        p.push(Box::new(OffsetFilter {
            tx_offset: 1,
            rx_offset: 1,
            name: "off",
        }));
        assert!(p.apply_tx(&[]).is_empty());
        assert!(p.apply_rx(&[]).is_empty());
    }
}
