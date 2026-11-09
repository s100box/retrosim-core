// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Shared helpers for bus-attached card implementations.
//!
//! Provides [`MemoryRange`] and [`IoPortRange`] to eliminate duplicated
//! address-checking boilerplate across card crates, plus the
//! `impl_interrupt_source!` macro for daisy-chain delegation.

use crate::card::IoAddrDecode;

/// A contiguous memory address range with offset calculation.
///
/// Used by RAM, ROM, and similar cards that claim a region of the
/// 32-bit physical address space.
///
/// # Example
/// ```
/// use retrosim_core::card_helpers::MemoryRange;
/// let range = MemoryRange::new(0xF000, 0x1000);
/// assert_eq!(range.offset(0xF000), Some(0));
/// assert_eq!(range.offset(0xF100), Some(0x100));
/// assert_eq!(range.offset(0xE000), None);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct MemoryRange {
    base: u32,
    /// Inclusive end address.
    end: u32,
}

impl MemoryRange {
    /// Create a new memory range starting at `base` with `size` bytes.
    ///
    /// # Panics
    /// Panics if `size` is zero or the range extends past the 32-bit address space.
    pub fn new(base: u32, size: usize) -> Self {
        assert!(size > 0, "MemoryRange size must be > 0");
        let last_offset =
            u32::try_from(size - 1).expect("MemoryRange exceeds the 32-bit address space");
        let end = base
            .checked_add(last_offset)
            .expect("MemoryRange exceeds the 32-bit address space");
        Self { base, end }
    }

    /// Return the byte offset if `addr` falls within this range.
    #[inline]
    pub fn offset(&self, addr: u32) -> Option<usize> {
        if addr >= self.base && addr <= self.end {
            Some((addr - self.base) as usize)
        } else {
            None
        }
    }

    /// Check whether `addr` falls within this range.
    #[inline]
    pub fn contains(&self, addr: u32) -> bool {
        addr >= self.base && addr <= self.end
    }

    /// The base address.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// The size in bytes.
    pub fn size(&self) -> usize {
        // Convert before adding: a full 32-bit range has 2^32 bytes on a
        // 64-bit host, even though its inclusive last address fits in u32.
        (self.end - self.base) as usize + 1
    }
}

/// An I/O port range with address decoding and offset calculation.
///
/// Encapsulates the common pattern of masking an address via
/// [`IoAddrDecode`], subtracting the base port, and checking the result
/// falls within a port count.
///
/// # Example
/// ```
/// use retrosim_core::card_helpers::IoPortRange;
/// use retrosim_core::card::IoAddrDecode;
/// let range = IoPortRange::new(0x40, 4, IoAddrDecode::Bits8);
/// assert_eq!(range.offset(0x40), Some(0));
/// assert_eq!(range.offset(0x43), Some(3));
/// assert_eq!(range.offset(0x44), None);
/// // With 8-bit decode, port 0x140 aliases to 0x40:
/// assert_eq!(range.offset(0x140), Some(0));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct IoPortRange {
    base_masked: u32,
    count: u32,
    decode: IoAddrDecode,
}

impl IoPortRange {
    /// Create a new I/O port range.
    ///
    /// - `base_port`: the first port number
    /// - `count`: number of consecutive ports
    /// - `decode`: address decoding width
    ///
    /// A zero count is empty. Addresses are masked before comparison; a range
    /// that reaches the end of the decoded space is clipped rather than wrapped.
    ///
    /// # Panics
    ///
    /// Panics if `count` exceeds 256, since [`Self::offset`] returns a byte offset.
    pub fn new(base_port: u16, count: u32, decode: IoAddrDecode) -> Self {
        assert!(count <= 256, "IoPortRange count must fit byte offsets");
        Self {
            base_masked: decode.mask(u32::from(base_port)),
            count,
            decode,
        }
    }

    /// Return the port offset (0-based) if `addr` maps to this range.
    #[inline]
    pub fn offset(&self, addr: u32) -> Option<u8> {
        let offset = self.decode.mask(addr).checked_sub(self.base_masked)?;
        (offset < self.count).then_some(offset as u8)
    }

    /// Check whether `addr` maps to any port in this range.
    #[inline]
    pub fn contains(&self, addr: u32) -> bool {
        self.offset(addr).is_some()
    }
}

/// Generate an [`InterruptSource`](crate::chip::InterruptSource) implementation
/// that delegates to a chip field.
///
/// Eliminates the repetitive 5-method delegation pattern used by CTC, DART,
/// SIO, PIO, and other interrupt-capable card wrappers.
///
/// The `ieo()` method is automatically derived as `!self.field.ius_method()`.
///
/// # Usage
/// ```
/// use retrosim_core::impl_interrupt_source;
/// use retrosim_core::chip::InterruptSource;
///
/// // A chip exposing the four methods the macro delegates to.
/// struct Ctc { irq: bool, ius: bool }
/// impl Ctc {
///     fn any_channel_interrupting(&self) -> bool { self.irq }
///     fn acknowledge(&mut self) -> u8 { self.ius = true; 0x10 }
///     fn reti(&mut self) { self.ius = false; }
///     fn ius_set(&self) -> bool { self.ius }
/// }
///
/// struct MyCard { ctc: Ctc }
/// impl_interrupt_source!(MyCard, ctc,
///     pending     = any_channel_interrupting,
///     acknowledge = acknowledge,
///     reti        = reti,
///     ius         = ius_set,
/// );
///
/// let mut card = MyCard { ctc: Ctc { irq: true, ius: false } };
/// assert!(card.interrupt_pending());
/// assert!(card.ieo());                       // downstream enabled before service
/// assert_eq!(card.acknowledge(), 0x10);
/// assert!(card.under_service() && !card.ieo()); // IUS set, downstream blocked
/// card.reti_notify();
/// assert!(!card.under_service());
/// ```
#[macro_export]
macro_rules! impl_interrupt_source {
    ($card:ty, $field:ident,
     pending     = $pending:ident,
     acknowledge = $ack:ident,
     reti        = $reti:ident,
     ius         = $ius:ident $(,)?
    ) => {
        impl $crate::chip::InterruptSource for $card {
            fn interrupt_pending(&self) -> bool {
                self.$field.$pending()
            }

            fn acknowledge(&mut self) -> u8 {
                self.$field.$ack()
            }

            fn reti_notify(&mut self) {
                self.$field.$reti()
            }

            fn under_service(&self) -> bool {
                self.$field.$ius()
            }

            fn ieo(&self) -> bool {
                !self.$field.$ius()
            }
        }
    };
}

// ---------------------------------------------------------------------------
// UART TX output buffer
// ---------------------------------------------------------------------------

use std::collections::VecDeque;

/// A transmit output buffer for UART card implementations.
///
/// Wraps a [`VecDeque<u8>`] with the standard pattern used by all UART-bearing
/// cards: bytes are pushed during `tick()` (from chip TX queues) and drained
/// in `uart_read()` (for transport/MCP consumption).
///
/// # Example
/// ```
/// use retrosim_core::card_helpers::UartTxBuffer;
/// let mut buf = UartTxBuffer::new();
/// buf.push(b'H');
/// buf.extend([b'i', b'!']);
/// assert!(!buf.is_empty());
/// let data = buf.drain();
/// assert_eq!(data, b"Hi!");
/// assert!(buf.is_empty());
/// ```
#[derive(Debug, Clone, Default)]
pub struct UartTxBuffer {
    buf: VecDeque<u8>,
}

impl UartTxBuffer {
    /// Create an empty TX buffer.
    pub fn new() -> Self {
        Self {
            buf: VecDeque::new(),
        }
    }

    /// Push a single byte into the buffer.
    #[inline]
    pub fn push(&mut self, byte: u8) {
        self.buf.push_back(byte);
    }

    /// Push multiple bytes into the buffer.
    pub fn extend(&mut self, bytes: impl IntoIterator<Item = u8>) {
        self.buf.extend(bytes);
    }

    /// Drain all buffered bytes and return them as a `Vec<u8>`.
    ///
    /// This is the standard implementation for [`Card::uart_read`](crate::card::Card::uart_read).
    pub fn drain(&mut self) -> Vec<u8> {
        self.buf.drain(..).collect()
    }

    /// Check if the buffer has any pending bytes.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Return the number of buffered bytes.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Clear all buffered bytes without returning them.
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

// ---------------------------------------------------------------------------
// Disk validation helpers
// ---------------------------------------------------------------------------

use crate::error::CardResult;

/// Validate that `drive_type` matches the expected value.
///
/// Returns `Err` with a descriptive message if mismatched.
///
/// # Example
/// ```
/// use retrosim_core::card_helpers::validate_drive_type;
/// assert!(validate_drive_type("floppy", "floppy").is_ok());
/// assert!(validate_drive_type("hard", "floppy").is_err());
/// ```
pub fn validate_drive_type(drive_type: &str, expected: &str) -> CardResult<()> {
    if drive_type != expected {
        return Err(format!("drive_type must be '{expected}', got '{drive_type}'").into());
    }
    Ok(())
}

/// Validate that `drive_num` is within `0..max_drives`.
///
/// Returns `Err` with a descriptive message if out of range.
///
/// # Example
/// ```
/// use retrosim_core::card_helpers::validate_drive_num;
/// assert!(validate_drive_num("floppy", 0, 4).is_ok());
/// assert!(validate_drive_num("floppy", 4, 4).is_err());
/// ```
pub fn validate_drive_num(drive_type: &str, drive_num: usize, max_drives: usize) -> CardResult<()> {
    if drive_num >= max_drives {
        if max_drives == 0 {
            return Err(format!("{drive_type} has no configured drives").into());
        }
        return Err(format!(
            "{drive_type} drive {drive_num} out of range (0-{})",
            max_drives - 1
        )
        .into());
    }
    Ok(())
}

/// Validate drive type and number in one call.
///
/// Combines [`validate_drive_type`] and [`validate_drive_num`].
pub fn validate_drive(
    drive_type: &str,
    expected_type: &str,
    drive_num: usize,
    max_drives: usize,
) -> CardResult<()> {
    validate_drive_type(drive_type, expected_type)?;
    validate_drive_num(drive_type, drive_num, max_drives)
}

// ---------------------------------------------------------------------------
// Dual-channel UART port name resolution
// ---------------------------------------------------------------------------

/// Resolve a port name to a channel index for dual-channel UART cards.
///
/// Compares `port` against two sets of names (configurable name + default name
/// for each channel) and returns `Some(0)` for channel A, `Some(1)` for
/// channel B, or `None` if neither matches.
///
/// This eliminates the repeated if-else port matching pattern in cards like
/// `dart-card`, `sio-card`, and `altair-2sio`.
///
/// # Example
/// ```
/// use retrosim_core::card_helpers::resolve_dual_uart;
/// // Configurable name "CONSOLE" for channel A, "sio-a" as default:
/// assert_eq!(resolve_dual_uart("CONSOLE", "CONSOLE", "sio-a", "MODEM", "sio-b"), Some(0));
/// assert_eq!(resolve_dual_uart("sio-a", "CONSOLE", "sio-a", "MODEM", "sio-b"), Some(0));
/// assert_eq!(resolve_dual_uart("MODEM", "CONSOLE", "sio-a", "MODEM", "sio-b"), Some(1));
/// assert_eq!(resolve_dual_uart("unknown", "CONSOLE", "sio-a", "MODEM", "sio-b"), None);
/// ```
#[inline]
pub fn resolve_dual_uart(
    port: &str,
    name_a: &str,
    default_a: &str,
    name_b: &str,
    default_b: &str,
) -> Option<usize> {
    if port == name_a || port == default_a {
        Some(0)
    } else if port == name_b || port == default_b {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MemoryRange tests ───────────────────────────────────────────

    #[test]
    fn memory_range_basic() {
        let r = MemoryRange::new(0x1000, 0x100);
        assert_eq!(r.offset(0x1000), Some(0));
        assert_eq!(r.offset(0x10FF), Some(0xFF));
        assert_eq!(r.offset(0x0FFF), None);
        assert_eq!(r.offset(0x1100), None);
    }

    #[test]
    fn memory_range_base_zero() {
        let r = MemoryRange::new(0, 0x10000);
        assert_eq!(r.offset(0), Some(0));
        assert_eq!(r.offset(0xFFFF), Some(0xFFFF));
        assert_eq!(r.offset(0x10000), None);
    }

    #[test]
    fn memory_range_contains() {
        let r = MemoryRange::new(0x8000, 0x4000);
        assert!(r.contains(0x8000));
        assert!(r.contains(0xBFFF));
        assert!(!r.contains(0x7FFF));
        assert!(!r.contains(0xC000));
    }

    #[test]
    fn memory_range_size() {
        let r = MemoryRange::new(0xF000, 0x1000);
        assert_eq!(r.size(), 0x1000);
        assert_eq!(r.base(), 0xF000);
    }

    #[test]
    #[should_panic(expected = "size must be > 0")]
    fn memory_range_zero_size_panics() {
        MemoryRange::new(0, 0);
    }

    #[test]
    fn memory_range_can_include_the_last_physical_address() {
        let range = MemoryRange::new(u32::MAX, 1);
        assert_eq!(range.offset(u32::MAX), Some(0));
        assert_eq!(range.size(), 1);
        assert!(!range.contains(u32::MAX - 1));
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn memory_range_can_cover_the_complete_32_bit_address_space() {
        let size = 1usize << 32;
        let range = MemoryRange::new(0, size);
        assert_eq!(range.size(), size);
        assert_eq!(range.offset(u32::MAX), Some(size - 1));
    }

    #[test]
    #[should_panic(expected = "exceeds the 32-bit address space")]
    fn memory_range_rejects_an_end_address_that_would_wrap() {
        MemoryRange::new(u32::MAX, 2);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    #[should_panic(expected = "exceeds the 32-bit address space")]
    fn memory_range_rejects_sizes_that_would_be_truncated() {
        MemoryRange::new(0, (1usize << 32) + 1);
    }

    // ── IoPortRange tests ───────────────────────────────────────────

    #[test]
    fn io_port_range_8bit_decode() {
        let r = IoPortRange::new(0x40, 4, IoAddrDecode::Bits8);
        assert_eq!(r.offset(0x40), Some(0));
        assert_eq!(r.offset(0x41), Some(1));
        assert_eq!(r.offset(0x43), Some(3));
        assert_eq!(r.offset(0x44), None);
        assert_eq!(r.offset(0x3F), None);
        // 8-bit aliasing
        assert_eq!(r.offset(0x140), Some(0));
        assert_eq!(r.offset(0xFF40), Some(0));
    }

    #[test]
    fn io_port_range_16bit_decode() {
        let r = IoPortRange::new(0x40, 4, IoAddrDecode::Bits16);
        assert_eq!(r.offset(0x40), Some(0));
        assert_eq!(r.offset(0x43), Some(3));
        // 16-bit: 0x140 is NOT aliased
        assert_eq!(r.offset(0x140), None);
    }

    #[test]
    fn io_port_range_single_port() {
        let r = IoPortRange::new(0xFE, 1, IoAddrDecode::Bits8);
        assert_eq!(r.offset(0xFE), Some(0));
        assert_eq!(r.offset(0xFF), None);
        assert_eq!(r.offset(0xFD), None);
    }

    #[test]
    fn io_port_range_contains() {
        let r = IoPortRange::new(0x20, 8, IoAddrDecode::Bits8);
        assert!(r.contains(0x20));
        assert!(r.contains(0x27));
        assert!(!r.contains(0x28));
        assert!(!r.contains(0x1F));
    }

    #[test]
    fn io_port_range_offsets_cover_one_byte_without_aliasing() {
        let range = IoPortRange::new(0x100, 256, IoAddrDecode::Bits16);
        assert_eq!(range.offset(0x100), Some(0));
        assert_eq!(range.offset(0x1ff), Some(255));
        assert!(!range.contains(0x200));
        assert!(!range.contains(0xff));
        assert!(!IoPortRange::new(0, 0, IoAddrDecode::Bits8).contains(0));
    }

    #[test]
    #[should_panic(expected = "count must fit byte offsets")]
    fn io_port_range_rejects_counts_that_would_alias_offsets() {
        IoPortRange::new(0, 257, IoAddrDecode::Bits16);
    }

    #[test]
    fn io_port_ranges_do_not_wrap_at_the_decode_boundary() {
        let range = IoPortRange::new(0xfe, 4, IoAddrDecode::Bits8);
        assert_eq!(range.offset(0xfe), Some(0));
        assert_eq!(range.offset(0xff), Some(1));
        assert!(!range.contains(0));
    }

    // ── impl_interrupt_source macro test ────────────────────────────

    /// Minimal chip mock for testing the macro.
    struct MockChip {
        pending: bool,
        ius: bool,
        last_vector: u8,
    }

    impl MockChip {
        fn is_pending(&self) -> bool {
            self.pending
        }
        fn acknowledge(&mut self) -> u8 {
            self.pending = false;
            self.ius = true;
            self.last_vector
        }
        fn reti(&mut self) {
            self.ius = false;
        }
        fn ius_set(&self) -> bool {
            self.ius
        }
    }

    struct MockCard {
        chip: MockChip,
    }

    impl_interrupt_source!(
        MockCard,
        chip,
        pending = is_pending,
        acknowledge = acknowledge,
        reti = reti,
        ius = ius_set,
    );

    #[test]
    fn interrupt_source_macro_delegation() {
        use crate::chip::InterruptSource;

        let mut card = MockCard {
            chip: MockChip {
                pending: true,
                ius: false,
                last_vector: 0x42,
            },
        };

        assert!(card.interrupt_pending());
        assert!(!card.under_service());
        assert!(card.ieo());

        let vec = card.acknowledge();
        assert_eq!(vec, 0x42);
        assert!(!card.interrupt_pending());
        assert!(card.under_service());
        assert!(!card.ieo());

        card.reti_notify();
        assert!(!card.under_service());
        assert!(card.ieo());
    }

    // ── UartTxBuffer tests ────────────────────────────────────────────

    #[test]
    fn uart_tx_buffer_new_is_empty() {
        let buf = UartTxBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn uart_tx_buffer_push_and_drain() {
        let mut buf = UartTxBuffer::new();
        buf.push(b'A');
        buf.push(b'B');
        assert!(!buf.is_empty());
        assert_eq!(buf.len(), 2);
        let data = buf.drain();
        assert_eq!(data, b"AB");
        assert!(buf.is_empty());
    }

    #[test]
    fn uart_tx_buffer_extend() {
        let mut buf = UartTxBuffer::new();
        buf.extend(b"Hello".iter().copied());
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.drain(), b"Hello");
    }

    #[test]
    fn uart_tx_buffer_drain_empty() {
        let mut buf = UartTxBuffer::new();
        let data = buf.drain();
        assert!(data.is_empty());
    }

    #[test]
    fn uart_tx_buffer_clear() {
        let mut buf = UartTxBuffer::new();
        buf.extend([1, 2, 3]);
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn uart_tx_buffer_default() {
        let buf = UartTxBuffer::default();
        assert!(buf.is_empty());
    }

    #[test]
    fn uart_tx_buffer_multiple_drains() {
        let mut buf = UartTxBuffer::new();
        buf.push(b'X');
        assert_eq!(buf.drain(), b"X");
        // Second drain should be empty
        assert!(buf.drain().is_empty());
        // Push more after drain
        buf.push(b'Y');
        assert_eq!(buf.drain(), b"Y");
    }

    // ── Disk validation helpers ──────────────────────────────────────

    #[test]
    fn validate_drive_type_ok() {
        assert!(validate_drive_type("floppy", "floppy").is_ok());
        assert!(validate_drive_type("hard", "hard").is_ok());
    }

    #[test]
    fn validate_drive_type_mismatch() {
        let err = validate_drive_type("hard", "floppy").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("floppy"));
        assert!(msg.contains("hard"));
    }

    #[test]
    fn validate_drive_num_ok() {
        assert!(validate_drive_num("floppy", 0, 4).is_ok());
        assert!(validate_drive_num("floppy", 3, 4).is_ok());
    }

    #[test]
    fn validate_drive_num_out_of_range() {
        let err = validate_drive_num("floppy", 4, 4).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("out of range"));
        assert!(msg.contains("0-3"));
    }

    #[test]
    fn no_configured_drives_is_an_error_without_integer_underflow() {
        for drive in [0, usize::MAX] {
            let error = validate_drive_num("floppy", drive, 0).unwrap_err();
            assert!(error.to_string().contains("no configured drives"));
        }
    }

    #[test]
    fn validate_drive_combined() {
        assert!(validate_drive("floppy", "floppy", 0, 4).is_ok());
        assert!(validate_drive("hard", "floppy", 0, 4).is_err());
        assert!(validate_drive("floppy", "floppy", 4, 4).is_err());
    }

    // ── Dual-channel UART resolution ─────────────────────────────────

    #[test]
    fn resolve_dual_uart_channel_a() {
        assert_eq!(
            resolve_dual_uart("CONSOLE", "CONSOLE", "sio-a", "MODEM", "sio-b"),
            Some(0)
        );
        assert_eq!(
            resolve_dual_uart("sio-a", "CONSOLE", "sio-a", "MODEM", "sio-b"),
            Some(0)
        );
    }

    #[test]
    fn resolve_dual_uart_channel_b() {
        assert_eq!(
            resolve_dual_uart("MODEM", "CONSOLE", "sio-a", "MODEM", "sio-b"),
            Some(1)
        );
        assert_eq!(
            resolve_dual_uart("sio-b", "CONSOLE", "sio-a", "MODEM", "sio-b"),
            Some(1)
        );
    }

    #[test]
    fn resolve_dual_uart_no_match() {
        assert_eq!(
            resolve_dual_uart("unknown", "CONSOLE", "sio-a", "MODEM", "sio-b"),
            None
        );
    }
}
