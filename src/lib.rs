// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! `retrosim-core` — Bus-agnostic foundation for retro system simulators.
//!
//! This crate contains traits and types that are shared across different
//! bus architectures (S-100, Unibus/Q-bus, etc.). It provides the common
//! vocabulary used by CPU, chip, card, transport, and debugger subsystems
//! without tying them to any specific bus implementation.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bus;
pub mod bus_context;
pub mod bus_master;
pub mod calendar;
pub mod card;
pub mod card_helpers;
pub mod chip;
pub mod cpu;
pub mod cpu_bus_helpers;
pub mod debugger;
pub mod disasm;
pub mod error;
pub mod machine;
pub mod perf;
pub mod serial;
pub mod serial_filter;
pub mod transport;
pub mod wallclock;

// ── Top-level re-exports ───────────────────────────────────────────────

pub use bus::{Bus, PhysicalBus};
pub use bus_master::{BusMaster, TransferBus};
pub use card::{
    AccessKind, BusAccess, Card, CardInfo, ConfigOption, DiskInfo, IoAddrDecode, IoRange,
    LogTargetInfo, MemRange, SlotId,
};
pub use chip::{Chip, ChipSignals, InterruptSource};
pub use cpu::{eval_condition, ConditionFlags, Cpu, Registers, RegistersDyn, TStates};
pub use debugger::{Breakpoint, Debugger, DebuggerState, TraceEntry, WatchHit, WatchKind};
pub use disasm::{disassemble_n, DisasmResult, Disassembler};
pub use error::{CardError, CardResult};
pub use machine::{CancellationToken, RunResult, StopReason};
pub use perf::{PerfSnapshot, PerfTracker, WallProportionalBudget};
pub use serial::{ModemInputs, ModemOutputs, SerialParams, SerialParity, SerialStopBits};
pub use serial_filter::{FilterPipeline, SerialFilter};
pub use transport::{ModemControl, Transport, TransportError, TransportInfo};
