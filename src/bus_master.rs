// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Bus mastering traits for DMA controllers and other bus-master devices.

/// A bus interface for DMA transfers.
///
/// Provides memory and I/O read/write operations that a bus master
/// can use while it holds the bus. Implemented by the system bus
/// (or an adapter) to dispatch transfers through the card manager.
pub trait TransferBus {
    /// Read a byte from memory at `addr`.
    fn mem_read(&mut self, addr: u32) -> u8;
    /// Write a byte to memory at `addr`.
    fn mem_write(&mut self, addr: u32, val: u8);
    /// Read a byte from an I/O port at `port`.
    fn io_read(&mut self, port: u16) -> u8;
    /// Write a byte to an I/O port at `port`.
    fn io_write(&mut self, port: u16, val: u8);
}

/// A card that can request and hold the bus for DMA transfers.
///
/// Bus masters (such as the Z80 DMA) assert BUSREQ to stall the CPU,
/// then perform memory-to-memory, memory-to-I/O, or I/O-to-memory
/// transfers while holding the bus.
pub trait BusMaster: Send + Sync {
    /// Whether this device is currently requesting the bus.
    fn bus_request(&self) -> bool;

    /// Perform one transfer cycle while holding the bus.
    ///
    /// Called repeatedly while [`bus_request`](BusMaster::bus_request) returns `true`.
    /// Returns the number of T-states consumed by this transfer cycle.
    fn tick_as_master(&mut self, bus: &mut dyn TransferBus) -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_bus_is_object_safe() {
        fn _accept(_: &dyn TransferBus) {}
        _accept(&FakeBus::default());
    }

    #[test]
    fn bus_master_is_object_safe() {
        fn _accept(_: &dyn BusMaster) {}
        _accept(&CopyMaster {
            src: 0,
            dst: 0,
            pending: false,
        });
    }

    /// Tiny RAM + I/O backing store implementing [`TransferBus`].
    #[derive(Default)]
    struct FakeBus {
        mem: std::collections::HashMap<u32, u8>,
        io: std::collections::HashMap<u16, u8>,
    }

    impl TransferBus for FakeBus {
        fn mem_read(&mut self, addr: u32) -> u8 {
            self.mem.get(&addr).copied().unwrap_or(0)
        }
        fn mem_write(&mut self, addr: u32, val: u8) {
            self.mem.insert(addr, val);
        }
        fn io_read(&mut self, port: u16) -> u8 {
            self.io.get(&port).copied().unwrap_or(0)
        }
        fn io_write(&mut self, port: u16, val: u8) {
            self.io.insert(port, val);
        }
    }

    /// One-shot DMA master that copies a single byte mem->mem, then drops
    /// its bus request.
    struct CopyMaster {
        src: u32,
        dst: u32,
        pending: bool,
    }

    impl BusMaster for CopyMaster {
        fn bus_request(&self) -> bool {
            self.pending
        }
        fn tick_as_master(&mut self, bus: &mut dyn TransferBus) -> u32 {
            let b = bus.mem_read(self.src);
            bus.mem_write(self.dst, b);
            self.pending = false;
            4
        }
    }

    #[test]
    fn transfer_bus_round_trips_mem_and_io() {
        let mut bus = FakeBus::default();
        assert_eq!(bus.mem_read(0x1000), 0); // default
        bus.mem_write(0x1000, 0xAB);
        assert_eq!(bus.mem_read(0x1000), 0xAB);
        bus.io_write(0x20, 0xCD);
        assert_eq!(bus.io_read(0x20), 0xCD);
        assert_eq!(bus.io_read(0x21), 0); // default
    }

    #[test]
    fn bus_master_drives_a_transfer_then_releases() {
        let mut bus = FakeBus::default();
        bus.mem_write(0x4000, 0x5A);
        let mut master = CopyMaster {
            src: 0x4000,
            dst: 0x8000,
            pending: true,
        };
        assert!(master.bus_request());
        let t = master.tick_as_master(&mut bus);
        assert_eq!(t, 4);
        assert_eq!(bus.mem_read(0x8000), 0x5A);
        assert!(!master.bus_request());
    }
}
