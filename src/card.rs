// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Card abstraction for bus-attached peripheral cards.

use crate::bus_master::BusMaster;
use crate::chip::InterruptSource;
pub use crate::error::{CardError, CardResult};
pub use crate::serial::{ModemInputs, ModemOutputs, SerialParams};
use serde::{Deserialize, Serialize};
use std::fmt;

/// I/O address decoding width for bus cards.
///
/// Most S-100 cards only decode the lower 8 bits of the I/O address.
/// Some newer cards (and non-S-100 peripherals) decode the full 16-bit address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoAddrDecode {
    /// Decode only the lower 8 bits of the I/O address (typical S-100).
    Bits8,
    /// Decode the full 16-bit I/O address.
    Bits16,
}

impl IoAddrDecode {
    /// Apply the address mask for port comparison.
    ///
    /// Accepts a `u32` address but only the lower 8 or 16 bits are
    /// relevant for I/O port decoding.
    pub fn mask(self, addr: u32) -> u32 {
        match self {
            IoAddrDecode::Bits8 => addr & 0xFF,
            IoAddrDecode::Bits16 => addr & 0xFFFF,
        }
    }
}

/// Unique identifier for a card slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlotId(pub u32);

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "slot:{}", self.0)
    }
}

/// A memory address range (inclusive on both ends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemRange {
    /// Start address.
    pub start: u32,
    /// End address (inclusive).
    pub end: u32,
    /// Human-readable description of what this memory range is used for.
    pub description: String,
}

/// An I/O address range (inclusive on both ends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoRange {
    /// Start port address.
    pub start: u32,
    /// End port address (inclusive).
    pub end: u32,
    /// Human-readable description of what this I/O range is used for.
    pub description: String,
}

/// Describes a card's identity and the address ranges it claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardInfo {
    /// Human-readable card name.
    pub name: String,
    /// Card description.
    pub description: String,
    /// Slot this card occupies.
    pub slot: SlotId,
    /// Memory address ranges this card responds to.
    pub mem_ranges: Vec<MemRange>,
    /// I/O port ranges this card responds to.
    pub io_ranges: Vec<IoRange>,
}

/// Kind of bus access being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    /// Memory read.
    MemRead,
    /// Memory write.
    MemWrite,
    /// I/O read.
    IoRead,
    /// I/O write.
    IoWrite,
}

/// Describes a single bus access operation.
#[derive(Debug, Clone, Copy)]
pub struct BusAccess<A: Copy> {
    /// Target address.
    pub addr: A,
    /// Kind of access.
    pub kind: AccessKind,
}

/// Trait for a bus-attached peripheral card.
///
/// Cards claim memory and I/O address ranges. The system bus dispatches
/// read/write operations to cards; a card returns `Some(data)` if it claims
/// the address, or `None` to let the bus try the next card.
pub trait Card<A: Copy>: Send + Sync {
    /// Card name for display purposes.
    fn name(&self) -> &str;

    /// Full card description including claimed address ranges.
    fn describe(&self) -> CardInfo;

    /// Read from memory. Returns `Some(byte)` if this card claims `addr`.
    ///
    /// Default returns `None` (card does not respond to memory reads).
    fn mem_read(&mut self, _addr: A) -> Option<u8> {
        None
    }

    /// Read from memory without side effects (non-mutating).
    ///
    /// Used by the trace/debugger system to peek at instruction bytes without
    /// triggering watchpoints or I/O side effects. Default returns `None`.
    /// Cards backed by simple memory (RAM, ROM) should override this.
    fn peek_mem(&self, _addr: A) -> Option<u8> {
        None
    }

    /// Write to memory. Returns `Some(())` if this card claims `addr`.
    ///
    /// Default returns `None` (card does not respond to memory writes).
    fn mem_write(&mut self, _addr: A, _val: u8) -> Option<()> {
        None
    }

    /// Read from I/O space. Returns `Some(byte)` if this card claims the port.
    ///
    /// Default returns `None` (card does not respond to I/O reads).
    fn io_read(&mut self, _addr: A) -> Option<u8> {
        None
    }

    /// Write to I/O space. Returns `Some(())` if this card claims the port.
    ///
    /// Default returns `None` (card does not respond to I/O writes).
    fn io_write(&mut self, _addr: A, _val: u8) -> Option<()> {
        None
    }

    /// Returns `true` if this card performs real work in [`tick()`](Self::tick).
    ///
    /// The bus uses this to build an optimized tick list, skipping cards
    /// whose `tick()` is a no-op (the default).  Cards that maintain
    /// timers, motor timeouts, baud rate generators, or any other
    /// time-dependent state **must** override this to return `true`.
    fn needs_tick(&self) -> bool {
        false
    }

    /// Advance the card's internal clock by `t_states` T-states.
    ///
    /// Default is a no-op. Cards with timers, counters, or timed
    /// operations should override this.
    fn tick(&mut self, _t_states: u32) {}

    /// Reset the card to its power-on state.
    ///
    /// Called when the system is reset (e.g., MCP `reset` tool).
    /// Cards should re-enable boot ROMs, clear interrupt state,
    /// and reset on-board chips to their default configuration.
    /// Disk images remain attached.  Default is a no-op.
    fn reset(&mut self) {}

    /// Return this card as an interrupt source, if it supports interrupts.
    ///
    /// Default returns `None`. Cards that participate in the daisy chain
    /// should override this.
    fn as_interrupt_source(&self) -> Option<&dyn InterruptSource> {
        None
    }

    /// Return this card as a mutable interrupt source, if it supports interrupts.
    fn as_interrupt_source_mut(&mut self) -> Option<&mut dyn InterruptSource> {
        None
    }

    /// Return this card as a bus master, if it supports DMA transfers.
    ///
    /// Default returns `None`. Cards with DMA controllers should override
    /// this to return a reference to their [`BusMaster`] implementation.
    fn as_bus_master(&self) -> Option<&dyn BusMaster> {
        None
    }

    /// Return this card as a mutable bus master, if it supports DMA transfers.
    fn as_bus_master_mut(&mut self) -> Option<&mut dyn BusMaster> {
        None
    }

    /// List the names of console (serial) ports this card exposes.
    ///
    /// Returns an empty slice by default. Cards with serial channels
    /// (DART, SIO, UART) should override this to return port names
    /// like `["SIO-A", "SIO-B"]`.
    fn uart_names(&self) -> &[&str] {
        &[]
    }

    /// Read pending output bytes from a named console port.
    ///
    /// Drains any bytes the card has transmitted since the last call.
    /// Returns an empty vec if the port name is unknown or has no output.
    fn uart_read(&mut self, _port: &str) -> Vec<u8> {
        Vec::new()
    }

    /// Write input bytes to a named console port (simulates keyboard input).
    ///
    /// Injects bytes into the card's receive path. Does nothing if the
    /// port name is unknown.
    fn uart_write(&mut self, _port: &str, _data: &[u8]) {}

    /// Read the guest UART's modem output signals for a named port.
    ///
    /// Default returns [`ModemOutputs::default()`] (DTR=false, RTS=false).
    /// Cards wrapping UARTs should override to return the chip's
    /// DTR/RTS state from its control register.
    fn uart_modem_out(&self, _port: &str) -> ModemOutputs {
        ModemOutputs::default()
    }

    /// Read the guest UART's modem input signals for a named port.
    ///
    /// Default returns [`ModemInputs::default()`] (CTS/DSR/DCD asserted, RI false).
    /// These reflect what the guest UART sees on its input pins.
    fn uart_modem_in(&self, _port: &str) -> ModemInputs {
        ModemInputs::default()
    }

    /// Set the guest UART's modem input signals for a named port.
    ///
    /// Used by the transport layer to update CTS/DSR/DCD/RI based on
    /// remote modem state or loopback configuration.
    /// Default is a no-op; cards with modem-aware UARTs should override.
    fn uart_set_modem_in(&mut self, _port: &str, _signals: ModemInputs) {}

    /// Enable or disable hardware loopback on a named port.
    ///
    /// When enabled, TX data is immediately injected back into the RX path
    /// during the card's `tick()`, simulating a physical RS-232 loopback
    /// connector. Default is a no-op.
    fn uart_set_loopback(&mut self, _port: &str, _enabled: bool) {}

    /// Return the guest UART's current serial line parameters.
    ///
    /// Default returns `None` (non-serial cards, or cards that don't
    /// track serial config).  Cards wrapping UARTs should override this
    /// to return the chip's configured baud rate, data bits, parity,
    /// and stop bits.
    fn uart_serial_params(&self, _port: &str) -> Option<SerialParams> {
        None
    }

    /// Notify a display-bearing UART port that the host terminal has been
    /// reset to default video attributes (e.g. `ESC[0m` was sent).
    ///
    /// Called when the debug monitor is entered or exited, so the card can
    /// synchronize its tracked ANSI state (reverse video, enhancement mode,
    /// etc.) with the actual terminal state. The default implementation is
    /// a no-op.
    fn uart_display_reset(&mut self, _port: &str) {}

    /// Return log target names this card's crate emits via `tracing`.
    ///
    /// Each entry is a [`LogTargetInfo`] with the Rust module-path target string
    /// and a short human-readable description. The default returns an empty list.
    /// Cards should override to list the crate and sub-module targets they use.
    fn log_targets(&self) -> Vec<LogTargetInfo> {
        Vec::new()
    }

    /// Return a human-readable summary of the card's current runtime configuration.
    ///
    /// This is used by the MCP server and CLI to display card state beyond
    /// what `describe()` provides. Default returns an empty string.
    fn config_summary(&self) -> String {
        String::new()
    }

    /// Return a list of configuration parameters this card supports.
    ///
    /// Default returns an empty list.
    fn config_options(&self) -> Vec<ConfigOption> {
        Vec::new()
    }

    /// Serialize the card's current configuration to a JSON value.
    ///
    /// Returns `None` if the card does not support config serialization.
    fn save_config(&self) -> Option<serde_json::Value> {
        None
    }

    /// Return the card type name used in configuration files.
    ///
    /// Returns `None` if the card does not support config serialization.
    fn card_type_name(&self) -> Option<&str> {
        None
    }

    /// Apply runtime configuration changes to this card.
    ///
    /// Accepts a JSON object with card-specific parameters. Default
    /// returns an error indicating the card does not support runtime
    /// configuration.
    fn apply_config(&mut self, _params: &serde_json::Value) -> CardResult<()> {
        Err(CardError::NotSupported)
    }

    /// List disk drives managed by this card.
    ///
    /// Default returns an empty list (card has no disk drives).
    fn disk_list(&self) -> Vec<DiskInfo> {
        Vec::new()
    }

    /// Set hard disk geometry for a drive before attaching an image.
    ///
    /// Must be called before [`disk_attach`](Card::disk_attach) for cards
    /// whose disk images need an explicit geometry (e.g., raw flat files
    /// that lack embedded geometry metadata).
    ///
    /// Default: silently succeeds (most cards have fixed geometry or
    /// derive it from the image format).
    fn disk_set_geometry(
        &mut self,
        _drive_num: usize,
        _cylinders: u16,
        _heads: u8,
        _sectors_per_track: u16,
        _sector_bytes: u16,
    ) -> CardResult<()> {
        Ok(())
    }

    /// Attach a disk image file to a drive on this card.
    fn disk_attach(
        &mut self,
        _drive_type: &str,
        _drive_num: usize,
        _path: &str,
        _read_only: bool,
    ) -> CardResult<()> {
        Err(CardError::NotSupported)
    }

    /// Detach (unmount) a disk image from a drive on this card.
    fn disk_detach(&mut self, _drive_type: &str, _drive_num: usize) -> CardResult<()> {
        Err(CardError::NotSupported)
    }

    /// Flush (save) a disk image back to its source file.
    fn disk_flush(&mut self, _drive_type: &str, _drive_num: usize) -> CardResult<()> {
        Err(CardError::NotSupported)
    }

    /// Flush all mounted disk images back to their source files.
    fn disk_flush_all(&mut self) -> CardResult<()> {
        Err(CardError::NotSupported)
    }

    /// Return the base log target string for this card.
    ///
    /// Uses a bus-agnostic `retrosim::` prefix so this foundation crate
    /// stays independent of any particular consumer.
    fn log_target_base(&self) -> String {
        let card_type = self.card_type_name().unwrap_or("unknown");
        let slot = self.describe().slot.0;
        format!("retrosim::{card_type}::{slot}")
    }
}

/// Information about a disk drive slot on a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Drive category (e.g., "floppy", "hd").
    pub drive_type: String,
    /// Drive number within its category.
    pub drive_num: usize,
    /// Currently mounted image path, if any.
    pub image_path: Option<String>,
    /// Whether the drive is mounted read-only.
    pub read_only: bool,
    /// Human-readable description (e.g., "8\" SSSD", "10MB Winchester").
    pub description: String,
    /// Optional user-assigned alias name (e.g., "FLOPPY0", "DISK1").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

/// Describes a single configuration parameter on a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOption {
    /// Parameter name (e.g. "rom", "drive0", "bank").
    pub name: String,
    /// Human-readable description of what this parameter controls.
    pub description: String,
    /// Current value as a string.
    pub current_value: String,
    /// Whether this parameter is read-only at runtime.
    pub read_only: bool,
}

/// Information about a log target emitted by a card or chip.
///
/// Fields use [`Cow<'static, str>`](std::borrow::Cow) so that cards can return static string
/// literals without allocating, while still supporting dynamic target names
/// when needed.
#[derive(Debug, Clone)]
pub struct LogTargetInfo {
    /// The tracing target string (Rust module path, e.g., `ibc_megastar::smd`).
    pub target: std::borrow::Cow<'static, str>,
    /// Short human-readable description of what this target logs.
    pub description: std::borrow::Cow<'static, str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_id_is_copy_eq_hash() {
        let a = SlotId(1);
        let b = a; // Copy
        assert_eq!(a, b); // Eq

        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(a); // Hash
        assert!(set.contains(&b));
    }

    #[test]
    fn slot_id_display() {
        assert_eq!(format!("{}", SlotId(42)), "slot:42");
    }

    #[test]
    fn card_trait_is_object_safe() {
        fn _accept(_: &dyn Card<u32>) {}
        _accept(&MinimalCard { slot: SlotId(0) });
    }

    #[test]
    fn mem_range_equality() {
        let a = MemRange {
            start: 0x0000,
            end: 0x3FFF,
            description: "test".into(),
        };
        let b = MemRange {
            start: 0x0000,
            end: 0x3FFF,
            description: "test".into(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn io_addr_decode_mask() {
        assert_eq!(IoAddrDecode::Bits8.mask(0x1234), 0x34);
        assert_eq!(IoAddrDecode::Bits16.mask(0x12_3456), 0x3456);
    }

    #[test]
    fn io_addr_decode_serde_round_trip() {
        for d in [IoAddrDecode::Bits8, IoAddrDecode::Bits16] {
            let json = serde_json::to_string(&d).unwrap();
            let back: IoAddrDecode = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }

    #[test]
    fn access_kind_and_bus_access() {
        assert_eq!(AccessKind::MemRead, AccessKind::MemRead);
        assert_ne!(AccessKind::MemRead, AccessKind::IoWrite);
        let acc = BusAccess {
            addr: 0x40u32,
            kind: AccessKind::IoWrite,
        };
        let copy = acc; // Copy
        assert_eq!(copy.addr, 0x40);
        assert_eq!(copy.kind, AccessKind::IoWrite);
    }

    #[test]
    fn card_info_serde_round_trip() {
        let info = CardInfo {
            name: "demo".into(),
            description: "a demo card".into(),
            slot: SlotId(3),
            mem_ranges: vec![MemRange {
                start: 0,
                end: 0x3FFF,
                description: "ROM".into(),
            }],
            io_ranges: vec![IoRange {
                start: 0x10,
                end: 0x1F,
                description: "ports".into(),
            }],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: CardInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "demo");
        assert_eq!(back.slot, SlotId(3));
        assert_eq!(back.mem_ranges, info.mem_ranges);
        assert_eq!(back.io_ranges, info.io_ranges);
    }

    #[test]
    fn disk_info_alias_skipped_when_none() {
        let d = DiskInfo {
            drive_type: "floppy".into(),
            drive_num: 0,
            image_path: None,
            read_only: false,
            description: "8\" SSSD".into(),
            alias: None,
        };
        // alias is skip_serializing_if = None, so it must be absent.
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("alias"));
        let back: DiskInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.alias, None);
        assert_eq!(back.drive_type, "floppy");
    }

    #[test]
    fn config_option_serde_round_trip() {
        let opt = ConfigOption {
            name: "rom".into(),
            description: "boot ROM image".into(),
            current_value: "monitor.bin".into(),
            read_only: false,
        };
        let json = serde_json::to_string(&opt).unwrap();
        let back: ConfigOption = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "rom");
        assert_eq!(back.current_value, "monitor.bin");
    }

    /// A card that overrides only the two required methods, so every other
    /// trait method falls through to its default implementation.
    struct MinimalCard {
        slot: SlotId,
    }

    impl Card<u32> for MinimalCard {
        fn name(&self) -> &str {
            "minimal"
        }
        fn describe(&self) -> CardInfo {
            CardInfo {
                name: "minimal".into(),
                description: "test card".into(),
                slot: self.slot,
                mem_ranges: Vec::new(),
                io_ranges: Vec::new(),
            }
        }
    }

    #[test]
    fn card_defaults_are_inert() {
        let mut c = MinimalCard { slot: SlotId(7) };

        assert_eq!(c.name(), "minimal");
        assert_eq!(c.describe().slot, SlotId(7));

        // Bus access defaults: nothing is claimed.
        assert_eq!(c.mem_read(0), None);
        assert_eq!(c.peek_mem(0), None);
        assert_eq!(c.mem_write(0, 0), None);
        assert_eq!(c.io_read(0), None);
        assert_eq!(c.io_write(0, 0), None);

        // Scheduling / lifecycle defaults.
        assert!(!c.needs_tick());
        c.tick(100); // no-op
        c.reset(); // no-op

        // Capability probes default to absent.
        assert!(c.as_interrupt_source().is_none());
        assert!(c.as_interrupt_source_mut().is_none());
        assert!(c.as_bus_master().is_none());
        assert!(c.as_bus_master_mut().is_none());

        // UART surface defaults.
        assert!(c.uart_names().is_empty());
        assert!(c.uart_read("X").is_empty());
        c.uart_write("X", b"hi"); // no-op
        assert_eq!(c.uart_modem_out("X"), ModemOutputs::default());
        assert_eq!(c.uart_modem_in("X"), ModemInputs::default());
        c.uart_set_modem_in("X", ModemInputs::default()); // no-op
        c.uart_set_loopback("X", true); // no-op
        assert!(c.uart_serial_params("X").is_none());
        c.uart_display_reset("X"); // no-op

        // Logging / config / type defaults.
        assert!(c.log_targets().is_empty());
        assert!(c.config_summary().is_empty());
        assert!(c.config_options().is_empty());
        assert!(c.save_config().is_none());
        assert!(c.card_type_name().is_none());
        assert!(matches!(
            c.apply_config(&serde_json::json!({})),
            Err(CardError::NotSupported)
        ));

        // Disk surface defaults.
        assert!(c.disk_list().is_empty());
        assert!(c.disk_set_geometry(0, 80, 2, 18, 512).is_ok());
        assert!(matches!(
            c.disk_attach("floppy", 0, "a.img", false),
            Err(CardError::NotSupported)
        ));
        assert!(matches!(
            c.disk_detach("floppy", 0),
            Err(CardError::NotSupported)
        ));
        assert!(matches!(
            c.disk_flush("floppy", 0),
            Err(CardError::NotSupported)
        ));
        assert!(matches!(c.disk_flush_all(), Err(CardError::NotSupported)));

        // log_target_base falls back to "unknown" when card_type_name is None.
        assert_eq!(c.log_target_base(), "retrosim::unknown::7");
    }
}
