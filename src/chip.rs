// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Chip and interrupt source abstractions.

/// Signals from a chip to the bus or CPU.
#[derive(Debug, Clone, Default)]
pub struct ChipSignals {
    /// Whether the chip is requesting an interrupt.
    pub interrupt_pending: bool,
    /// Whether the chip is requesting the bus (DMA BUSREQ signal).
    pub bus_request: bool,
}

/// Trait for a peripheral chip (e.g., Z80-CTC, Z80-PIO).
///
/// Chips have internal registers accessed by a register address
/// and advance their state on clock ticks.
pub trait Chip: Send + Sync {
    /// Chip name for display purposes.
    fn name(&self) -> &str;

    /// Read a chip register at `addr`.
    fn read(&mut self, addr: u8) -> u8;

    /// Write `val` to a chip register at `addr`.
    fn write(&mut self, addr: u8, val: u8);

    /// Advance the chip's internal clock by `t_states` T-states.
    fn tick(&mut self, t_states: u32);
}

/// Trait for devices that participate in the Z80 daisy-chain interrupt system.
///
/// Each interrupt source has a priority position in the daisy chain.
/// When interrupting, it drives a vector byte onto the data bus during
/// the interrupt acknowledge cycle. The RETI instruction propagates down
/// the chain to clear the "interrupt under service" (IUS) flag.
pub trait InterruptSource: Send + Sync {
    /// Returns `true` if this source has a pending interrupt request.
    fn interrupt_pending(&self) -> bool;

    /// Acknowledge the interrupt: set IUS and return the interrupt vector.
    fn acknowledge(&mut self) -> u8;

    /// Notify that a RETI instruction was executed.
    ///
    /// If this source is currently under service, it clears its IUS flag,
    /// allowing lower-priority devices to interrupt.
    fn reti_notify(&mut self);

    /// Returns `true` if this source is currently under service (IUS set).
    fn under_service(&self) -> bool;

    /// Interrupt Enable Out: `true` if lower-priority devices may interrupt.
    ///
    /// Returns `false` when this device (or a higher-priority channel within
    /// it) has its IUS flag set, blocking all downstream devices in the chain.
    fn ieo(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_signals_default() {
        let sig = ChipSignals::default();
        assert!(!sig.interrupt_pending);
        assert!(!sig.bus_request);
    }

    #[test]
    fn chip_trait_is_object_safe() {
        fn _accept(_: &dyn Chip) {}
        _accept(&CounterChip {
            regs: [0; 2],
            ticks: 0,
        });
    }

    #[test]
    fn interrupt_source_is_object_safe() {
        fn _accept(_: &dyn InterruptSource) {}
        _accept(&Source {
            pending: false,
            ius: false,
            vector: 0,
        });
    }

    #[test]
    fn chip_signals_clone_and_debug() {
        let sig = ChipSignals {
            interrupt_pending: true,
            bus_request: true,
        };
        let copy = sig.clone();
        assert!(copy.interrupt_pending && copy.bus_request);
        assert!(format!("{sig:?}").contains("ChipSignals"));
    }

    /// A trivial 2-register chip that counts ticks.
    struct CounterChip {
        regs: [u8; 2],
        ticks: u32,
    }

    impl Chip for CounterChip {
        fn name(&self) -> &str {
            "counter"
        }
        fn read(&mut self, addr: u8) -> u8 {
            self.regs[addr as usize & 1]
        }
        fn write(&mut self, addr: u8, val: u8) {
            self.regs[addr as usize & 1] = val;
        }
        fn tick(&mut self, t_states: u32) {
            self.ticks += t_states;
        }
    }

    #[test]
    fn chip_read_write_tick() {
        let mut c = CounterChip {
            regs: [0; 2],
            ticks: 0,
        };
        assert_eq!(c.name(), "counter");
        c.write(0, 0x12);
        c.write(1, 0x34);
        assert_eq!(c.read(0), 0x12);
        assert_eq!(c.read(1), 0x34);
        c.tick(7);
        c.tick(3);
        assert_eq!(c.ticks, 10);
    }

    /// Single-channel interrupt source modeling the IUS/IEO daisy-chain
    /// state machine.
    struct Source {
        pending: bool,
        ius: bool,
        vector: u8,
    }

    impl InterruptSource for Source {
        fn interrupt_pending(&self) -> bool {
            self.pending && !self.ius
        }
        fn acknowledge(&mut self) -> u8 {
            self.ius = true;
            self.pending = false;
            self.vector
        }
        fn reti_notify(&mut self) {
            self.ius = false;
        }
        fn under_service(&self) -> bool {
            self.ius
        }
        fn ieo(&self) -> bool {
            !self.ius
        }
    }

    #[test]
    fn interrupt_source_acknowledge_and_reti_cycle() {
        let mut s = Source {
            pending: true,
            ius: false,
            vector: 0x30,
        };
        assert!(s.interrupt_pending());
        assert!(s.ieo()); // downstream allowed before service
        assert!(!s.under_service());

        assert_eq!(s.acknowledge(), 0x30);
        assert!(s.under_service());
        assert!(!s.ieo()); // downstream blocked while under service
        assert!(!s.interrupt_pending()); // masked by IUS

        s.reti_notify();
        assert!(!s.under_service());
        assert!(s.ieo());
    }
}
