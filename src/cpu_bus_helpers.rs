// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Shared CPU bus helper functions for 16-bit little-endian architectures.
//!
//! These free functions eliminate duplicated `fetch_byte`, `fetch_word`,
//! `read_word`, `write_word`, `push`, and `pop` implementations between
//! CPU crates (Z80, 8080, Z180, etc.) that share identical little-endian,
//! 16-bit address bus logic.
//!
//! These helpers require an [`Infallible`] bus, so unwrapping a bus result
//! cannot fail. Addresses, program counters, and stack pointers wrap at 16 bits.

use crate::bus::Bus;
use std::convert::Infallible;

/// Fetch the next byte from memory at `*pc` and advance `*pc` by 1.
#[inline]
pub fn fetch_byte(pc: &mut u16, bus: &mut (dyn Bus<Addr = u16, Error = Infallible> + '_)) -> u8 {
    let addr = *pc;
    *pc = pc.wrapping_add(1);
    bus.read(addr).unwrap()
}

/// Fetch a 16-bit little-endian word from memory at `*pc` and advance `*pc` by 2.
#[inline]
pub fn fetch_word(pc: &mut u16, bus: &mut (dyn Bus<Addr = u16, Error = Infallible> + '_)) -> u16 {
    let lo = fetch_byte(pc, bus) as u16;
    let hi = fetch_byte(pc, bus) as u16;
    (hi << 8) | lo
}

/// Read a 16-bit little-endian word from the given address.
#[inline]
pub fn read_word(bus: &mut (dyn Bus<Addr = u16, Error = Infallible> + '_), addr: u16) -> u16 {
    let lo = bus.read(addr).unwrap() as u16;
    let hi = bus.read(addr.wrapping_add(1)).unwrap() as u16;
    (hi << 8) | lo
}

/// Write a 16-bit little-endian word to the given address.
#[inline]
pub fn write_word(bus: &mut (dyn Bus<Addr = u16, Error = Infallible> + '_), addr: u16, val: u16) {
    bus.write(addr, val as u8).unwrap();
    bus.write(addr.wrapping_add(1), (val >> 8) as u8).unwrap();
}

/// Push a 16-bit value onto the stack (pre-decrement `*sp`).
#[inline]
pub fn push(sp: &mut u16, bus: &mut (dyn Bus<Addr = u16, Error = Infallible> + '_), val: u16) {
    *sp = sp.wrapping_sub(1);
    bus.write(*sp, (val >> 8) as u8).unwrap();
    *sp = sp.wrapping_sub(1);
    bus.write(*sp, val as u8).unwrap();
}

/// Pop a 16-bit value from the stack (post-increment `*sp`).
#[inline]
pub fn pop(sp: &mut u16, bus: &mut (dyn Bus<Addr = u16, Error = Infallible> + '_)) -> u16 {
    let lo = bus.read(*sp).unwrap() as u16;
    *sp = sp.wrapping_add(1);
    let hi = bus.read(*sp).unwrap() as u16;
    *sp = sp.wrapping_add(1);
    (hi << 8) | lo
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 64 KiB RAM-backed bus for exercising the little-endian helpers.
    struct RamBus {
        mem: Vec<u8>,
    }

    impl RamBus {
        fn new() -> Self {
            Self {
                mem: vec![0u8; 0x1_0000],
            }
        }
    }

    impl Bus for RamBus {
        type Addr = u16;
        type Error = Infallible;

        fn read(&mut self, addr: u16) -> Result<u8, Infallible> {
            Ok(self.mem[addr as usize])
        }
        fn write(&mut self, addr: u16, val: u8) -> Result<(), Infallible> {
            self.mem[addr as usize] = val;
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
            false
        }
        fn int_acknowledge(&mut self) -> Option<u8> {
            None
        }
        fn reti_notify(&mut self) {}
    }

    #[test]
    fn ram_bus_io_and_control_lines() {
        // Exercise the non-memory bus surface the helpers don't touch.
        let mut bus = RamBus::new();
        assert_eq!(bus.io_read(0x10).unwrap(), 0);
        bus.io_write(0x10, 0xFF).unwrap();
        bus.tick(4);
        assert!(!bus.interrupt_pending());
        assert_eq!(bus.int_acknowledge(), None);
        bus.reti_notify();
    }

    #[test]
    fn fetch_byte_advances_pc() {
        let mut bus = RamBus::new();
        bus.mem[0x100] = 0x42;
        bus.mem[0x101] = 0x99;
        let mut pc = 0x100u16;
        assert_eq!(fetch_byte(&mut pc, &mut bus), 0x42);
        assert_eq!(pc, 0x101);
        assert_eq!(fetch_byte(&mut pc, &mut bus), 0x99);
        assert_eq!(pc, 0x102);
    }

    #[test]
    fn fetch_byte_wraps_pc() {
        let mut bus = RamBus::new();
        bus.mem[0xFFFF] = 0x7E;
        let mut pc = 0xFFFFu16;
        assert_eq!(fetch_byte(&mut pc, &mut bus), 0x7E);
        assert_eq!(pc, 0x0000);
    }

    #[test]
    fn fetch_word_is_little_endian() {
        let mut bus = RamBus::new();
        bus.mem[0x200] = 0x34; // lo
        bus.mem[0x201] = 0x12; // hi
        let mut pc = 0x200u16;
        assert_eq!(fetch_word(&mut pc, &mut bus), 0x1234);
        assert_eq!(pc, 0x202);
    }

    #[test]
    fn read_and_write_word_round_trip() {
        let mut bus = RamBus::new();
        write_word(&mut bus, 0x4000, 0xBEEF);
        assert_eq!(bus.mem[0x4000], 0xEF); // lo first
        assert_eq!(bus.mem[0x4001], 0xBE); // hi second
        assert_eq!(read_word(&mut bus, 0x4000), 0xBEEF);
    }

    #[test]
    fn read_word_wraps_high_byte() {
        let mut bus = RamBus::new();
        bus.mem[0xFFFF] = 0xCD; // lo
        bus.mem[0x0000] = 0xAB; // hi (wrapped)
        assert_eq!(read_word(&mut bus, 0xFFFF), 0xABCD);
    }

    #[test]
    fn push_then_pop_round_trips() {
        let mut bus = RamBus::new();
        let mut sp = 0x8000u16;
        push(&mut sp, &mut bus, 0x1234);
        // Pre-decrement by 2; hi at sp+1, lo at sp.
        assert_eq!(sp, 0x7FFE);
        assert_eq!(bus.mem[0x7FFE], 0x34);
        assert_eq!(bus.mem[0x7FFF], 0x12);
        assert_eq!(pop(&mut sp, &mut bus), 0x1234);
        assert_eq!(sp, 0x8000);
    }

    #[test]
    fn push_wraps_stack_pointer() {
        let mut bus = RamBus::new();
        let mut sp = 0x0000u16;
        push(&mut sp, &mut bus, 0xABCD);
        assert_eq!(sp, 0xFFFE);
        assert_eq!(bus.mem[0xFFFE], 0xCD);
        assert_eq!(bus.mem[0xFFFF], 0xAB);
        assert_eq!(pop(&mut sp, &mut bus), 0xABCD);
        assert_eq!(sp, 0x0000);
    }
}
