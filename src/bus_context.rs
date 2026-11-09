// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Shared bus context for the current CPU state.
//!
//! Provides a thread-local that holds the current CPU program counter,
//! updated by the machine execution loop before each
//! instruction step. Chip and card crates can read this value to include
//! the current PC in tracing output without changes to the `Card`
//! trait signature.
//!
//! The TLS write is gated behind an atomic flag ([`set_tracing_active`]) so
//! that the per-instruction overhead is near zero when no tracing subscriber
//! is installed.
//!
//! # Usage
//!
//! ```
//! use retrosim_core::bus_context;
//!
//! // The machine loop enables PC publishing and updates it before each step.
//! bus_context::set_tracing_active(true);
//! bus_context::set_current_pc(0x1234);
//!
//! // A card or chip then reads it to annotate trace output, e.g.
//! // `tracing::trace!(pc = format_args!("0x{:04X}", bus_context::current_pc()), ...)`.
//! assert_eq!(bus_context::current_pc(), 0x1234);
//! # bus_context::set_tracing_active(false);
//! ```

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag: set to `true` when a tracing subscriber that uses PC
/// context has been installed.  When `false`, [`set_current_pc`] is a
/// no-op, avoiding the TLS write on every instruction step.
static TRACING_ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static BUS_PC: Cell<u32> = const { Cell::new(0) };
    static BUS_SP: Cell<u32> = const { Cell::new(0) };
    static BUS_HL: Cell<u32> = const { Cell::new(0) };
    static BUS_DE: Cell<u32> = const { Cell::new(0) };
    static BUS_BC: Cell<u32> = const { Cell::new(0) };
    static BUS_IY: Cell<u32> = const { Cell::new(0) };
}

/// Enable or disable the per-instruction PC publish.
///
/// Call with `true` when a tracing subscriber is installed that reads
/// bus-context PC values.  Call with `false` to elide the TLS write
/// for maximum performance.
pub fn set_tracing_active(active: bool) {
    TRACING_ACTIVE.store(active, Ordering::Release);
}

/// Returns `true` if bus-context tracing is active.
pub fn is_tracing_active() -> bool {
    TRACING_ACTIVE.load(Ordering::Acquire)
}

/// Set the current CPU program counter.
///
/// Called by the machine execution loop before each CPU step so that
/// peripherals can include the PC in trace output.  Returns immediately
/// when tracing is not active ([`set_tracing_active`] was not called
/// with `true`).
#[inline]
pub fn set_current_pc(pc: u32) {
    if !TRACING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    BUS_PC.with(|p| p.set(pc));
}

/// Get the current CPU program counter.
///
/// Returns the PC value set by the most recent [`set_current_pc`] call.
/// Cards and chips call this inside tracing macros to annotate log entries
/// with the instruction address that triggered the bus access.
#[inline]
pub fn current_pc() -> u32 {
    BUS_PC.with(|p| p.get())
}

/// Set the current CPU stack pointer. Same TLS gating as [`set_current_pc`].
#[inline]
pub fn set_current_sp(sp: u32) {
    if !TRACING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    BUS_SP.with(|p| p.set(sp));
}

/// Get the current CPU stack pointer.
#[inline]
pub fn current_sp() -> u32 {
    BUS_SP.with(|p| p.get())
}

/// Set the current HL register pair (Z80) or equivalent. Same TLS gating
/// as [`set_current_pc`]. Used by service-call / system-call tracing where
/// the calling convention passes an argument pointer in HL.
#[inline]
pub fn set_current_hl(hl: u32) {
    if !TRACING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    BUS_HL.with(|p| p.set(hl));
}

/// Get the current HL register pair.
#[inline]
pub fn current_hl() -> u32 {
    BUS_HL.with(|p| p.get())
}

/// Set the current DE register pair (Z80) or equivalent.
#[inline]
pub fn set_current_de(de: u32) {
    if !TRACING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    BUS_DE.with(|p| p.set(de));
}

/// Get the current DE register pair.
#[inline]
pub fn current_de() -> u32 {
    BUS_DE.with(|p| p.get())
}

/// Set the current BC register pair (Z80) or equivalent.
#[inline]
pub fn set_current_bc(bc: u32) {
    if !TRACING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    BUS_BC.with(|p| p.set(bc));
}

/// Get the current BC register pair.
#[inline]
pub fn current_bc() -> u32 {
    BUS_BC.with(|p| p.get())
}

/// Set the current IY index register (Z80). OASIS uses IY to point at the
/// per-process System Communication Region (SCR), so tracing the SC
/// dispatcher needs it to resolve SCR-relative buffers.
#[inline]
pub fn set_current_iy(iy: u32) {
    if !TRACING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    BUS_IY.with(|p| p.set(iy));
}

/// Get the current IY index register.
#[inline]
pub fn current_iy() -> u32 {
    BUS_IY.with(|p| p.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The `TRACING_ACTIVE` flag is process-global, so tests that flip it
    /// must run one at a time — otherwise one test's "inactive" phase can
    /// race another's "active" phase. (TLS register cells are per-thread,
    /// and libtest gives each test a fresh thread, so only the flag races.)
    static GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn default_pc_is_zero() {
        // Thread-local starts at 0
        assert_eq!(current_pc(), 0);
    }

    #[test]
    fn set_and_get_pc() {
        let _guard = GUARD.lock().unwrap();
        // Enable tracing so set_current_pc actually writes.
        set_tracing_active(true);
        set_current_pc(0x1234);
        assert_eq!(current_pc(), 0x1234);
        set_current_pc(0xABCD);
        assert_eq!(current_pc(), 0xABCD);
        // Reset for other tests
        set_current_pc(0);
        set_tracing_active(false);
    }

    #[test]
    fn set_pc_is_noop_when_tracing_inactive() {
        let _guard = GUARD.lock().unwrap();
        set_tracing_active(true);
        set_current_pc(0x1234);
        set_tracing_active(false);
        set_current_pc(0x5678);
        assert_eq!(current_pc(), 0x1234);
    }

    #[test]
    fn tracing_active_flag_round_trips() {
        let _guard = GUARD.lock().unwrap();
        set_tracing_active(true);
        assert!(is_tracing_active());
        set_tracing_active(false);
        assert!(!is_tracing_active());
    }

    #[test]
    fn all_register_pairs_set_and_get_when_active() {
        let _guard = GUARD.lock().unwrap();
        set_tracing_active(true);
        set_current_sp(0x0100);
        set_current_hl(0x0202);
        set_current_de(0x0303);
        set_current_bc(0x0404);
        set_current_iy(0x0505);
        assert_eq!(current_sp(), 0x0100);
        assert_eq!(current_hl(), 0x0202);
        assert_eq!(current_de(), 0x0303);
        assert_eq!(current_bc(), 0x0404);
        assert_eq!(current_iy(), 0x0505);
        // Reset so this test doesn't leak state into others on this thread.
        set_current_sp(0);
        set_current_hl(0);
        set_current_de(0);
        set_current_bc(0);
        set_current_iy(0);
        set_tracing_active(false);
    }

    #[test]
    fn register_setters_are_noops_when_inactive() {
        let _guard = GUARD.lock().unwrap();
        // Seed known values while active.
        set_tracing_active(true);
        set_current_sp(0x11);
        set_current_hl(0x22);
        set_current_de(0x33);
        set_current_bc(0x44);
        set_current_iy(0x55);
        set_tracing_active(false);

        // These writes must be elided while tracing is inactive.
        set_current_sp(0xDEAD);
        set_current_hl(0xDEAD);
        set_current_de(0xDEAD);
        set_current_bc(0xDEAD);
        set_current_iy(0xDEAD);

        assert_eq!(current_sp(), 0x11);
        assert_eq!(current_hl(), 0x22);
        assert_eq!(current_de(), 0x33);
        assert_eq!(current_bc(), 0x44);
        assert_eq!(current_iy(), 0x55);
    }
}
