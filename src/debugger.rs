// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Debugger: breakpoints, watchpoints, and execution state tracking.

use std::collections::VecDeque;

/// Parse a breakpoint/watchpoint address from a string.
///
/// Accepts two forms:
///
/// 1. **Linear address** — a hexadecimal number, optionally prefixed with
///    `0x`/`0X`. Examples: `"3F8F"`, `"0x3F8F"`, `"ABCDE"`. This is the
///    natural form for flat-address CPUs (Z80, 8080, 68k, 6502) and is
///    also how segmented CPUs (8086) report their PC — the 8086's
///    `Registers::pc()` returns `(CS << 4) + IP` masked to the physical
///    address space.
///
/// 2. **Segment:offset** — two hexadecimal numbers separated by `:`.
///    Computed as `(segment << 4) + offset`. Natural for 8086 code: a
///    breakpoint set as `"0040:3F8F"` fires at linear `0x0438F` — the
///    same location an 8086 program refers to as `0040:3F8F`.
///
/// Case is ignored. A leading `0x` or `0X` prefix is accepted on either
/// half of a seg:off pair (e.g., `"0x0040:0x3F8F"`). Returns `None` on
/// parse failure or 32-bit addition overflow. The segment is limited to 16
/// bits; the offset may use the full 32-bit range. No CPU address mask is applied.
///
/// This parser is the single source of truth for breakpoint address
/// syntax across the monitor, DAP, and MCP interfaces so that the same
/// string produces the same breakpoint regardless of how it was entered.
///
/// # Examples
/// ```
/// use retrosim_core::debugger::parse_address;
/// assert_eq!(parse_address("3F8F"), Some(0x3F8F));
/// assert_eq!(parse_address("0x3F8F"), Some(0x3F8F));
/// assert_eq!(parse_address("0040:3F8F"), Some(0x0438F));
/// assert_eq!(parse_address("F000:FFF0"), Some(0xFFFF0));
/// assert_eq!(parse_address(""), None);
/// assert_eq!(parse_address("xyz"), None);
/// ```
pub fn parse_address(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if let Some(colon) = s.find(':') {
        let seg = parse_hex_with_optional_prefix(&s[..colon])?;
        let off = parse_hex_with_optional_prefix(&s[colon + 1..])?;
        // Segments are 16 bits; larger values are rejected.
        if seg > 0xFFFF {
            return None;
        }
        (seg << 4).checked_add(off)
    } else {
        parse_hex_with_optional_prefix(s)
    }
}

/// Parse a hex literal that may or may not have a `0x`/`0X` prefix.
fn parse_hex_with_optional_prefix(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let body = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if body.is_empty() {
        return None;
    }
    u32::from_str_radix(body, 16).ok()
}

/// A single trace log entry recording one executed instruction.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// Program counter at the time of execution.
    pub pc: u32,
    /// Opcode bytes (up to 4).
    pub bytes: [u8; 4],
    /// Number of valid opcode bytes in `bytes` (0–4).
    pub byte_count: u8,
    /// T-states consumed by this instruction.
    pub t_states: u32,
    /// Total T-clock at the time of execution.
    pub t_clock: u64,
}

/// Kind of memory watchpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchKind {
    /// Watch for reads at the address.
    Read,
    /// Watch for writes at the address.
    Write,
    /// Watch for both reads and writes.
    ReadWrite,
}

impl WatchKind {
    /// Returns `true` if this watch kind matches a read access.
    pub fn matches_read(self) -> bool {
        matches!(self, WatchKind::Read | WatchKind::ReadWrite)
    }

    /// Returns `true` if this watch kind matches a write access.
    pub fn matches_write(self) -> bool {
        matches!(self, WatchKind::Write | WatchKind::ReadWrite)
    }
}

/// Information about a watchpoint hit during execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchHit {
    /// The address that triggered the watchpoint.
    pub addr: u32,
    /// The kind of access that triggered the watchpoint.
    pub kind: WatchKind,
}

/// A breakpoint specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breakpoint {
    /// Break when the program counter reaches this address.
    Address(u32),
    /// Break at a source file location (requires symbol information).
    SourceLine {
        /// Source file path.
        file: String,
        /// Line number (1-based).
        line: u32,
    },
}

/// Current debugger execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebuggerState {
    /// Running freely until a stop condition.
    Running,
    /// Single-stepping one instruction at a time.
    Stepping,
    /// Paused, waiting for a debugger command.
    Paused,
}

/// Debugger for CPU execution control.
///
/// Stores breakpoints, watchpoints, and a trace ring buffer. The
/// consuming crate's run loop drives it.
pub struct Debugger {
    breakpoints: Vec<Breakpoint>,
    watchpoints: Vec<(u32, WatchKind)>,
    state: DebuggerState,
    trace_log: VecDeque<TraceEntry>,
    trace_capacity: usize,
    trace_enabled: bool,
}

impl Debugger {
    /// Default trace log capacity.
    const DEFAULT_TRACE_CAPACITY: usize = 4096;

    /// Create a new debugger in the [`DebuggerState::Running`] state.
    pub fn new() -> Self {
        Self {
            breakpoints: Vec::new(),
            watchpoints: Vec::new(),
            state: DebuggerState::Running,
            trace_log: VecDeque::new(),
            trace_capacity: Self::DEFAULT_TRACE_CAPACITY,
            trace_enabled: false,
        }
    }

    /// Returns `true` if any breakpoints are set.
    #[inline]
    pub fn has_breakpoints(&self) -> bool {
        !self.breakpoints.is_empty()
    }

    /// Check address breakpoints against the given program counter.
    ///
    /// Consumers must resolve source locations to addresses before execution.
    pub fn should_break(&self, pc: u32) -> bool {
        self.breakpoints
            .iter()
            .any(|bp| matches!(bp, Breakpoint::Address(a) if *a == pc))
    }

    /// Add a breakpoint. Returns `true` if the breakpoint was new.
    pub fn add_breakpoint(&mut self, bp: Breakpoint) -> bool {
        if self.breakpoints.contains(&bp) {
            false
        } else {
            self.breakpoints.push(bp);
            true
        }
    }

    /// Remove a breakpoint. Returns `true` if it was found and removed.
    pub fn remove_breakpoint(&mut self, bp: &Breakpoint) -> bool {
        if let Some(pos) = self.breakpoints.iter().position(|b| b == bp) {
            self.breakpoints.remove(pos);
            true
        } else {
            false
        }
    }

    /// Add a watchpoint at the given address.
    pub fn add_watchpoint(&mut self, addr: u32, kind: WatchKind) {
        self.watchpoints.push((addr, kind));
    }

    /// Get the current debugger state.
    pub fn state(&self) -> DebuggerState {
        self.state
    }

    /// Set the debugger state.
    pub fn set_state(&mut self, state: DebuggerState) {
        self.state = state;
    }

    /// Return a slice of all current breakpoints.
    pub fn breakpoints(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    /// Remove all breakpoints.
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Return a slice of all current watchpoints.
    pub fn watchpoints(&self) -> &[(u32, WatchKind)] {
        &self.watchpoints
    }

    /// Remove the first watchpoint at the given address; return whether found.
    pub fn remove_watchpoint(&mut self, addr: u32) -> bool {
        if let Some(pos) = self.watchpoints.iter().position(|(a, _)| *a == addr) {
            self.watchpoints.remove(pos);
            true
        } else {
            false
        }
    }

    /// Remove all watchpoints.
    pub fn clear_watchpoints(&mut self) {
        self.watchpoints.clear();
    }

    /// Enable or disable instruction tracing.
    pub fn set_trace_enabled(&mut self, enabled: bool) {
        self.trace_enabled = enabled;
    }

    /// Returns `true` if instruction tracing is enabled.
    pub fn trace_enabled(&self) -> bool {
        self.trace_enabled
    }

    /// Set the maximum number of trace entries to keep.
    /// Zero clears the log and disables retention without changing the trace flag.
    pub fn set_trace_capacity(&mut self, capacity: usize) {
        self.trace_capacity = capacity;
        while self.trace_log.len() > capacity {
            self.trace_log.pop_front();
        }
    }

    /// Record a trace entry. Drops the oldest if at capacity.
    pub fn record_trace(&mut self, entry: TraceEntry) {
        if !self.trace_enabled || self.trace_capacity == 0 {
            return;
        }
        if self.trace_log.len() >= self.trace_capacity {
            self.trace_log.pop_front();
        }
        self.trace_log.push_back(entry);
    }

    /// Get the trace log as a slice-like view.
    pub fn trace_log(&self) -> &VecDeque<TraceEntry> {
        &self.trace_log
    }

    /// Clear the trace log.
    pub fn clear_trace_log(&mut self) {
        self.trace_log.clear();
    }

    /// Get the most recent N trace entries (newest last).
    pub fn recent_trace(&self, n: usize) -> Vec<&TraceEntry> {
        let start = self.trace_log.len().saturating_sub(n);
        self.trace_log.iter().skip(start).collect()
    }

    /// Set the CPU type name for display (no-op; reserved for consumers).
    pub fn set_cpu_type(&mut self, _name: &str) {}

    /// Set the address width in bits for display (no-op; reserved for consumers).
    pub fn set_addr_width(&mut self, _bits: u8) {}
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debugger_default_state() {
        let dbg = Debugger::new();
        assert_eq!(dbg.state(), DebuggerState::Running);
        assert!(!dbg.should_break(0x0000));
    }

    #[test]
    fn add_remove_breakpoint() {
        let mut dbg = Debugger::new();
        let bp = Breakpoint::Address(0x1000);

        assert!(dbg.add_breakpoint(bp.clone()));
        assert!(!dbg.add_breakpoint(bp.clone())); // duplicate
        assert!(dbg.should_break(0x1000));
        assert!(!dbg.should_break(0x2000));

        assert!(dbg.remove_breakpoint(&bp));
        assert!(!dbg.should_break(0x1000));
        assert!(!dbg.remove_breakpoint(&bp)); // already removed
    }

    #[test]
    fn watchpoint_add_remove() {
        let mut dbg = Debugger::new();
        dbg.add_watchpoint(0xFF00, WatchKind::ReadWrite);
        dbg.add_watchpoint(0xFF01, WatchKind::Write);
        assert_eq!(dbg.watchpoints().len(), 2);
        assert_eq!(dbg.watchpoints()[0], (0xFF00, WatchKind::ReadWrite));
        assert_eq!(dbg.watchpoints()[1], (0xFF01, WatchKind::Write));

        assert!(dbg.remove_watchpoint(0xFF00));
        assert_eq!(dbg.watchpoints().len(), 1);
        assert!(!dbg.remove_watchpoint(0xFF00)); // already removed
    }

    #[test]
    fn clear_breakpoints_and_watchpoints() {
        let mut dbg = Debugger::new();
        dbg.add_breakpoint(Breakpoint::Address(0x100));
        dbg.add_breakpoint(Breakpoint::Address(0x200));
        dbg.add_watchpoint(0xFF00, WatchKind::Read);
        assert_eq!(dbg.breakpoints().len(), 2);
        assert_eq!(dbg.watchpoints().len(), 1);

        dbg.clear_breakpoints();
        assert!(dbg.breakpoints().is_empty());

        dbg.clear_watchpoints();
        assert!(dbg.watchpoints().is_empty());
    }

    fn make_trace_entry(pc: u32, t_clock: u64) -> TraceEntry {
        TraceEntry {
            pc,
            bytes: [0x00, 0, 0, 0],
            byte_count: 1,
            t_states: 4,
            t_clock,
        }
    }

    #[test]
    fn trace_disabled_by_default() {
        let mut dbg = Debugger::new();
        assert!(!dbg.trace_enabled());
        dbg.record_trace(make_trace_entry(0, 0));
        assert!(
            dbg.trace_log().is_empty(),
            "disabled trace should not record"
        );
    }

    #[test]
    fn trace_records_when_enabled() {
        let mut dbg = Debugger::new();
        dbg.set_trace_enabled(true);
        dbg.record_trace(make_trace_entry(0x0000, 0));
        dbg.record_trace(make_trace_entry(0x0001, 4));
        assert_eq!(dbg.trace_log().len(), 2);
        assert_eq!(dbg.trace_log()[0].pc, 0x0000);
        assert_eq!(dbg.trace_log()[1].pc, 0x0001);
    }

    #[test]
    fn trace_respects_capacity() {
        let mut dbg = Debugger::new();
        dbg.set_trace_enabled(true);
        dbg.set_trace_capacity(3);
        for i in 0..5 {
            dbg.record_trace(make_trace_entry(i, i as u64 * 4));
        }
        assert_eq!(dbg.trace_log().len(), 3);
        // Should have entries 2, 3, 4 (oldest dropped)
        assert_eq!(dbg.trace_log()[0].pc, 2);
        assert_eq!(dbg.trace_log()[2].pc, 4);
    }

    #[test]
    fn zero_trace_capacity_never_retains_an_entry() {
        let mut debugger = Debugger::new();
        debugger.set_trace_enabled(true);
        debugger.record_trace(make_trace_entry(1, 4));
        debugger.set_trace_capacity(0);
        debugger.record_trace(make_trace_entry(2, 8));
        assert!(debugger.trace_enabled());
        assert!(debugger.trace_log().is_empty());
        debugger.set_trace_capacity(1);
        debugger.record_trace(make_trace_entry(3, 12));
        assert_eq!(debugger.trace_log().len(), 1);
        assert_eq!(debugger.trace_log()[0].pc, 3);
    }

    #[test]
    fn recent_trace_returns_newest() {
        let mut dbg = Debugger::new();
        dbg.set_trace_enabled(true);
        for i in 0..10 {
            dbg.record_trace(make_trace_entry(i, i as u64 * 4));
        }
        let recent = dbg.recent_trace(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].pc, 7);
        assert_eq!(recent[1].pc, 8);
        assert_eq!(recent[2].pc, 9);
    }

    #[test]
    fn clear_trace_log() {
        let mut dbg = Debugger::new();
        dbg.set_trace_enabled(true);
        dbg.record_trace(make_trace_entry(0, 0));
        assert_eq!(dbg.trace_log().len(), 1);
        dbg.clear_trace_log();
        assert!(dbg.trace_log().is_empty());
    }

    #[test]
    fn has_breakpoints_and_state_transitions() {
        // Default::default() routes through new().
        let mut dbg = Debugger::default();
        assert!(!dbg.has_breakpoints());
        dbg.add_breakpoint(Breakpoint::Address(0x100));
        assert!(dbg.has_breakpoints());

        assert_eq!(dbg.state(), DebuggerState::Running);
        dbg.set_state(DebuggerState::Stepping);
        assert_eq!(dbg.state(), DebuggerState::Stepping);
        dbg.set_state(DebuggerState::Paused);
        assert_eq!(dbg.state(), DebuggerState::Paused);

        // No-op display setters must not panic.
        dbg.set_cpu_type("z80");
        dbg.set_addr_width(16);
    }

    #[test]
    fn shrinking_trace_capacity_drops_oldest() {
        let mut dbg = Debugger::new();
        dbg.set_trace_enabled(true);
        for i in 0..5 {
            dbg.record_trace(make_trace_entry(i, i as u64 * 4));
        }
        assert_eq!(dbg.trace_log().len(), 5);
        // Reducing capacity below current length trims from the front.
        dbg.set_trace_capacity(2);
        assert_eq!(dbg.trace_log().len(), 2);
        assert_eq!(dbg.trace_log()[0].pc, 3);
        assert_eq!(dbg.trace_log()[1].pc, 4);
    }

    #[test]
    fn watch_kind_matches_read() {
        assert!(WatchKind::Read.matches_read());
        assert!(!WatchKind::Read.matches_write());
        assert!(!WatchKind::Write.matches_read());
        assert!(WatchKind::Write.matches_write());
        assert!(WatchKind::ReadWrite.matches_read());
        assert!(WatchKind::ReadWrite.matches_write());
    }

    #[test]
    fn parse_address_linear_hex() {
        // Plain hex — default base.
        assert_eq!(parse_address("0"), Some(0));
        assert_eq!(parse_address("FF"), Some(0xFF));
        assert_eq!(parse_address("ff"), Some(0xFF));
        assert_eq!(parse_address("3F8F"), Some(0x3F8F));
        assert_eq!(parse_address("FFFF"), Some(0xFFFF));
        // 20-bit range (8086 physical).
        assert_eq!(parse_address("03F8F"), Some(0x03F8F));
        assert_eq!(parse_address("FFFFF"), Some(0xFFFFF));
        // 32-bit range (68k).
        assert_eq!(parse_address("DEADBEEF"), Some(0xDEAD_BEEF));
    }

    #[test]
    fn parse_address_with_0x_prefix() {
        assert_eq!(parse_address("0xFF"), Some(0xFF));
        assert_eq!(parse_address("0X3F8F"), Some(0x3F8F));
        assert_eq!(parse_address("0xFFFFF"), Some(0xFFFFF));
    }

    #[test]
    fn parse_address_segment_offset() {
        // (seg << 4) + off
        assert_eq!(parse_address("0000:0000"), Some(0x00000));
        // A nonzero segment contributes to the linear address.
        assert_eq!(parse_address("0040:3B8F"), Some(0x03F8F));
        // Another offset in the same segment.
        assert_eq!(parse_address("0040:74F1"), Some(0x078F1));
        // Reset vector: FFFF:0000 → linear 0xFFFF0.
        assert_eq!(parse_address("FFFF:0000"), Some(0xFFFF0));
        // No architecture-specific 20-bit mask is applied.
        assert_eq!(parse_address("FFFF:0010"), Some(0x100000));
    }

    #[test]
    fn parse_address_seg_off_with_0x_prefix() {
        // Both halves accept an optional 0x prefix.
        assert_eq!(parse_address("0x0040:0x3B8F"), Some(0x03F8F));
        assert_eq!(parse_address("0040:0x3B8F"), Some(0x03F8F));
        assert_eq!(parse_address("0x0040:3B8F"), Some(0x03F8F));
    }

    #[test]
    fn parse_address_trims_whitespace() {
        assert_eq!(parse_address("  3F8F  "), Some(0x3F8F));
        assert_eq!(parse_address("  0040:3B8F  "), Some(0x03F8F));
    }

    #[test]
    fn parse_address_both_forms_agree() {
        // The two forms should produce the same value for the same
        // linear address. This is the property that makes breakpoints
        // consistent between monitor, DAP, and MCP regardless of which
        // form the user types.
        assert_eq!(parse_address("03F8F"), parse_address("0040:3B8F"));
        assert_eq!(parse_address("078F1"), parse_address("0040:74F1"));
        assert_eq!(parse_address("FFFF0"), parse_address("FFFF:0000"));
    }

    #[test]
    fn parse_address_rejects_invalid_input() {
        assert_eq!(parse_address(""), None);
        assert_eq!(parse_address("   "), None);
        assert_eq!(parse_address("xyz"), None);
        assert_eq!(parse_address("0x"), None);
        assert_eq!(parse_address(":"), None);
        assert_eq!(parse_address("0040:"), None);
        assert_eq!(parse_address(":3B8F"), None);
        assert_eq!(parse_address("0040:xyz"), None);
        // Segment larger than 16 bits is not a valid 8086 segment.
        assert_eq!(parse_address("10000:0000"), None);
    }

    #[test]
    fn segmented_addresses_cannot_wrap_into_unrelated_breakpoints() {
        assert_eq!(parse_address("0000:FFFFFFFF"), Some(u32::MAX));
        assert_eq!(parse_address("0001:FFFFFFEF"), Some(u32::MAX));
        assert_eq!(parse_address("0001:FFFFFFF0"), None);
        assert_eq!(parse_address("FFFF:FFFFFFFF"), None);
    }

    #[test]
    fn watch_hit_equality() {
        let a = WatchHit {
            addr: 0x1000,
            kind: WatchKind::Write,
        };
        let b = WatchHit {
            addr: 0x1000,
            kind: WatchKind::Write,
        };
        let c = WatchHit {
            addr: 0x2000,
            kind: WatchKind::Read,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
