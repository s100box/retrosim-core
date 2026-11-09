// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Bus trait for CPU and peripheral communication.
//!
//! The `Bus` trait is generic over address and error types, allowing
//! different bus architectures (S-100, Unibus, etc.) to share the same
//! CPU and debugger infrastructure.

use crate::debugger::{WatchHit, WatchKind};
use std::fmt::{Debug, Display};
use std::hash::Hash;

/// Trait representing a system bus that CPUs and peripherals communicate over.
///
/// Generic over address type and error type:
/// - a fault-free 16-bit bus (Z80) uses `u16` / `std::convert::Infallible`
/// - a wider bus uses a larger address type (e.g. `u32`) and a real fault type
pub trait Bus: Send + Sync {
    /// Address type for this bus.
    type Addr: Copy + Debug + Display + Default + Ord + Hash + Send + Sync + 'static;

    /// Error type for bus faults. Use `std::convert::Infallible` for fault-free buses.
    type Error: Debug + Send + Sync + 'static;

    /// Read a byte from memory at `addr`.
    fn read(&mut self, addr: Self::Addr) -> Result<u8, Self::Error>;

    /// Write `val` to memory at `addr`.
    fn write(&mut self, addr: Self::Addr, val: u8) -> Result<(), Self::Error>;

    /// Read a byte from I/O space at `addr`.
    fn io_read(&mut self, addr: Self::Addr) -> Result<u8, Self::Error>;

    /// Write `val` to I/O space at `addr`.
    fn io_write(&mut self, addr: Self::Addr, val: u8) -> Result<(), Self::Error>;

    /// Read a byte from an external-data-memory address (distinct from
    /// program memory on CPUs with separate bus pins for the two — e.g.,
    /// MCS-48's `MOVX` path via /RD vs program-fetch via /PSEN).
    ///
    /// Default implementation delegates to [`Bus::read`] for CPUs that
    /// share a single address space; cards targeting split-bus chips
    /// override this to route data-memory accesses to MMIO without
    /// colliding with ROM addresses.
    fn ext_data_read(&mut self, addr: Self::Addr) -> Result<u8, Self::Error> {
        self.read(addr)
    }

    /// Write `val` to an external-data-memory address. See
    /// [`Bus::ext_data_read`] for why this is distinct from [`Bus::write`].
    fn ext_data_write(&mut self, addr: Self::Addr, val: u8) -> Result<(), Self::Error> {
        self.write(addr, val)
    }

    /// Advance the bus clock by `t_states` T-states.
    fn tick(&mut self, t_states: u32);

    /// Returns `true` if any device on the bus is requesting an interrupt.
    fn interrupt_pending(&self) -> bool;

    /// Acknowledge an interrupt and return the vector byte, if any.
    ///
    /// The bus walks the daisy chain to find the highest-priority requesting
    /// device, sets its IUS flag, and returns its interrupt vector.
    fn int_acknowledge(&mut self) -> Option<u8>;

    /// Notify peripherals that a RETI instruction has been executed.
    ///
    /// The bus propagates this down the daisy chain so the highest-priority
    /// device currently under service can clear its IUS flag.
    fn reti_notify(&mut self);

    /// Sync watchpoints from the debugger into the bus for hit detection.
    ///
    /// When watchpoints are active, the bus checks each memory read/write
    /// against the list and records the first hit. Default does nothing.
    fn set_watchpoints(&mut self, _watchpoints: &[(u32, WatchKind)]) {}

    /// Clear any pending watchpoint hit. Default does nothing.
    fn clear_watch_hit(&mut self) {}

    /// Take the watchpoint hit, if any. Default returns `None`.
    fn take_watch_hit(&mut self) -> Option<WatchHit> {
        None
    }

    /// Returns `true` if any bus master device is currently requesting the bus.
    ///
    /// When a bus master is active, the CPU is stalled (BUSREQ/BUSACK protocol)
    /// and the master performs DMA transfers. Default returns `false`.
    fn bus_master_active(&self) -> bool {
        false
    }

    /// Run one DMA transfer cycle for all active bus masters.
    ///
    /// Returns the total T-states consumed by bus master transfers.
    /// Default returns 0.
    fn tick_bus_masters(&mut self) -> u32 {
        0
    }
}

/// Extended bus trait for CPUs with physical address spaces wider than [`Bus::Addr`].
///
/// Used by CPUs that translate logical addresses to wider physical addresses:
/// - Intel 8086/8088: 16-bit segment:offset → 20-bit physical
/// - Z180: 16-bit logical → 20-bit physical via MMU (CBR/BBR/CBAR)
/// - MC68000: 16-bit S-100 port → 24-bit physical memory
///
/// This trait is dyn-safe: `&mut dyn PhysicalBus` can be used in function
/// pointers (e.g., 8086 opcode handler tables).
pub trait PhysicalBus: Bus<Addr = u16, Error = std::convert::Infallible> {
    /// Read a byte from a physical memory address (wider than `Addr`).
    fn mem_read_phys(&mut self, addr: u32) -> u8;

    /// Write a byte to a physical memory address (wider than `Addr`).
    fn mem_write_phys(&mut self, addr: u32, val: u8);

    /// Returns `true` if INT0 specifically is pending.
    ///
    /// Used by the Z8S180 emulator to distinguish INT0 (which obeys the
    /// IM mode) from other external interrupts that always use IM2
    /// vectoring.  Default delegates to [`Bus::interrupt_pending`].
    fn interrupt_pending_int0(&self) -> bool {
        self.interrupt_pending()
    }

    /// Returns `true` if INT1 specifically is pending, and likewise
    /// [`Self::interrupt_pending_int2`] for INT2.
    ///
    /// The Z8X180 has three maskable interrupt pins, and only INT0 is the
    /// Z80's.  INT1 and INT2 are always vectored through the on-chip IL
    /// register rather than through anything the bus supplies, and their
    /// acknowledge asserts neither `M1` nor `IORQ` — so, unlike INT0's, it
    /// is not a bus cycle a device can answer.  There is therefore nothing
    /// for a bus to return, and nothing but the pin to report: a bus that
    /// wires these two says so here, and one that does not (the S-100
    /// backplane, whose single `INT` line lands on INT0) leaves the default.
    ///
    /// ⚠ Default is `false`, NOT [`Bus::interrupt_pending`]: a bus that
    /// reports a generic pending interrupt is asserting INT0, which is what
    /// [`Self::interrupt_pending_int0`]'s default already says.  Delegating
    /// here as well would make one pin look like three.
    fn interrupt_pending_int1(&self) -> bool {
        false
    }

    /// Returns `true` if INT2 specifically is pending.
    /// See [`Self::interrupt_pending_int1`].
    fn interrupt_pending_int2(&self) -> bool {
        false
    }

    /// Take a pending CSIO external clock edge, if any.
    ///
    /// Returns `true` (and clears the flag) if the bus detected a
    /// CSIO_EXT_CLK toggle since the last call.  The Z180 CSIO uses
    /// this to count shift-register clock edges in external clock mode.
    /// Default returns `false` (no external clock source).
    fn take_csio_ext_clock(&mut self) -> bool {
        false
    }

    /// Record an interrupt-trace entry on the bus's structured trace ring
    /// buffer (if any).
    ///
    /// CPU implementations call this from their `dispatch_interrupt`
    /// path so that *every* interrupt — hardware vector, software
    /// `INT n`, divide-by-zero, single-step, breakpoint, INTO, double
    /// fault, etc. — is captured in a single place. The default is a
    /// no-op for buses that don't carry an interrupt trace ring buffer.
    /// `pc` is the linear address of the interrupted instruction (i.e.
    /// what the CPU would have executed next if the interrupt hadn't
    /// fired); `vector` is the IVT index.
    fn record_int_trace(&mut self, _pc: u32, _vector: u8) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    /// Minimal bus used only to exercise the provided default methods.
    #[derive(Default)]
    struct TestBus {
        mem: [u8; 4],
        io: [u8; 4],
        ticks: u32,
    }

    impl Bus for TestBus {
        type Addr = u16;
        type Error = Infallible;

        fn read(&mut self, addr: u16) -> Result<u8, Infallible> {
            Ok(self.mem[addr as usize % 4])
        }
        fn write(&mut self, addr: u16, val: u8) -> Result<(), Infallible> {
            self.mem[addr as usize % 4] = val;
            Ok(())
        }
        fn io_read(&mut self, addr: u16) -> Result<u8, Infallible> {
            Ok(self.io[addr as usize % 4])
        }
        fn io_write(&mut self, addr: u16, val: u8) -> Result<(), Infallible> {
            self.io[addr as usize % 4] = val;
            Ok(())
        }
        fn tick(&mut self, t_states: u32) {
            self.ticks += t_states;
        }
        fn interrupt_pending(&self) -> bool {
            false
        }
        fn int_acknowledge(&mut self) -> Option<u8> {
            None
        }
        fn reti_notify(&mut self) {}
    }

    impl PhysicalBus for TestBus {
        fn mem_read_phys(&mut self, addr: u32) -> u8 {
            self.mem[addr as usize % 4]
        }
        fn mem_write_phys(&mut self, addr: u32, val: u8) {
            self.mem[addr as usize % 4] = val;
        }
    }

    #[test]
    fn ext_data_access_defaults_to_main_memory() {
        let mut bus = TestBus::default();
        // ext_data_write should land in the same store read() sees, and vice versa.
        bus.ext_data_write(2, 0xAB).unwrap();
        assert_eq!(bus.read(2).unwrap(), 0xAB);
        bus.write(1, 0xCD).unwrap();
        assert_eq!(bus.ext_data_read(1).unwrap(), 0xCD);
    }

    #[test]
    fn watchpoint_defaults_are_inert() {
        let mut bus = TestBus::default();
        // Default impls are no-ops / None; calling them must not panic.
        bus.set_watchpoints(&[(0x10, WatchKind::Write)]);
        bus.clear_watch_hit();
        assert!(bus.take_watch_hit().is_none());
    }

    #[test]
    fn bus_master_defaults() {
        let mut bus = TestBus::default();
        assert!(!bus.bus_master_active());
        assert_eq!(bus.tick_bus_masters(), 0);
    }

    #[test]
    fn tick_accumulates() {
        let mut bus = TestBus::default();
        bus.tick(3);
        bus.tick(4);
        assert_eq!(bus.ticks, 7);
    }

    #[test]
    fn io_and_interrupt_passthrough() {
        let mut bus = TestBus::default();
        bus.io_write(1, 0x7F).unwrap();
        assert_eq!(bus.io_read(1).unwrap(), 0x7F);
        assert!(!bus.interrupt_pending());
        assert_eq!(bus.int_acknowledge(), None);
        bus.reti_notify();
    }

    /// A bus with a maskable interrupt standing, which is the only state in
    /// which the three `interrupt_pending_intN` defaults differ from each
    /// other. `TestBus` answers `false` to everything, so it cannot tell
    /// "INT0 delegates" from "INT1 is hard-wired false".
    #[derive(Default)]
    struct PendingBus;

    impl Bus for PendingBus {
        type Addr = u16;
        type Error = Infallible;

        fn read(&mut self, _addr: u16) -> Result<u8, Infallible> {
            Ok(0)
        }
        fn write(&mut self, _addr: u16, _val: u8) -> Result<(), Infallible> {
            Ok(())
        }
        fn io_read(&mut self, _addr: u16) -> Result<u8, Infallible> {
            Ok(0)
        }
        fn io_write(&mut self, _addr: u16, _val: u8) -> Result<(), Infallible> {
            Ok(())
        }
        fn tick(&mut self, _t_states: u32) {}
        fn interrupt_pending(&self) -> bool {
            true
        }
        fn int_acknowledge(&mut self) -> Option<u8> {
            Some(0xFF)
        }
        fn reti_notify(&mut self) {}
    }

    impl PhysicalBus for PendingBus {
        fn mem_read_phys(&mut self, _addr: u32) -> u8 {
            0
        }
        fn mem_write_phys(&mut self, _addr: u32, _val: u8) {}
    }

    /// A generic pending interrupt means INT0 and ONLY INT0. The Z8X180's
    /// other two pins are wired by buses that have them, and a bus that does
    /// not mention them has not asserted them -- delegating all three to
    /// `interrupt_pending` would make one line look like three.
    #[test]
    fn a_pending_interrupt_is_int0_and_not_int1_or_int2() {
        let bus = PendingBus;
        assert!(bus.interrupt_pending_int0());
        assert!(!bus.interrupt_pending_int1());
        assert!(!bus.interrupt_pending_int2());
    }

    #[test]
    fn physical_bus_defaults() {
        let mut bus = TestBus::default();
        // interrupt_pending_int0 delegates to interrupt_pending.
        assert!(!bus.interrupt_pending_int0());
        assert!(!bus.interrupt_pending_int1());
        assert!(!bus.interrupt_pending_int2());
        assert!(!bus.take_csio_ext_clock());
        // record_int_trace is a no-op by default.
        bus.record_int_trace(0x1234, 0x08);
        // Physical accessors round-trip through the same store.
        bus.mem_write_phys(3, 0x5A);
        assert_eq!(bus.mem_read_phys(3), 0x5A);
    }
}
