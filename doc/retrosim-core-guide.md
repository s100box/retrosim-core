# retrosim-core design

<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Howard M. Harte -->

[README](../README.md) · Source paths are relative to the repository root.

<a id="contents"></a>
**Contents**

- [1. Purpose and scope](#1-purpose-and-scope)
- [2. Bus interface](#2-bus-interface)
- [3. Card interface](#3-card-interface)
- [4. Chip interface](#4-chip-interface)
- [5. CPU interface](#5-cpu-interface)
- [6. Debugger](#6-debugger)
- [7. Transport layer](#7-transport-layer)
- [8. Serial filters](#8-serial-filters)
- [8.5. Performance instrumentation](#85-performance-instrumentation)
- [9. Execution control](#9-execution-control)
- [10. Building a new simulator](#10-building-a-new-simulator)
- [11. API conventions](#11-api-conventions)
- [12. Verification](#12-verification)

<a id="1--overview"></a>
## 1. Purpose and scope

`retrosim-core` provides the bus-agnostic trait definitions and types shared by all
retro system simulators in the retrosim family. It defines *what* a CPU, bus, card, chip,
debugger, and transport look like — but contains no bus-specific implementations.

Your simulator provides the bus-specific glue: how cards are dispatched, how interrupts
are arbitrated, and how addresses are decoded for your particular bus architecture.

<a id="11--what-retrosim-core-provides"></a>
### 1.1. What retrosim-core provides

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `bus` | `Bus`, `PhysicalBus` | Central interconnect trait — generic over address and error types |
| `bus_context` | `current_pc`, `set_current_pc`, … | Thread-local CPU register context for tracing |
| `bus_master` | `BusMaster`, `TransferBus` | DMA controller abstractions |
| `calendar` | `unix_to_broken`, `broken_to_unix` | Epoch ↔ broken-down date/time math |
| `card` | `Card<A>`, `CardInfo`, `SlotId` | Plug-in peripheral trait with address decode |
| `card_helpers` | `MemoryRange`, `IoPortRange`, `UartTxBuffer`, `impl_interrupt_source!` | Reusable helpers |
| `chip` | `Chip`, `ChipSignals`, `InterruptSource` | IC-level abstraction |
| `cpu` | `Cpu`, `Registers`, `RegistersDyn` | CPU trait hierarchy |
| `cpu_bus_helpers` | `fetch_byte`, `read_word`, `push`, `pop`, … | Little-endian 16-bit bus helpers |
| `debugger` | `Debugger`, `Breakpoint`, `WatchKind` | Breakpoints, watchpoints, trace log |
| `disasm` | `Disassembler`, `DisasmResult` | Disassembly interface |
| `error` | `CardError`, `CardResult` | Structured error types |
| `machine` | `StopReason`, `RunResult`, `CancellationToken` | Execution control |
| `perf` | `PerfTracker`, `WallProportionalBudget` | Wall-time vs sim-time accounting |
| `serial` | `SerialParams`, `ModemInputs`, `ModemOutputs` | Serial port configuration |
| `serial_filter` | `SerialFilter`, `FilterPipeline` | Serial I/O transformation chain |
| `transport` | `Transport`, `ModemControl`, `TransportInfo` | Character I/O backends |
| `wallclock` | `host_local_wall_secs`, `now_local` | Host wall-clock helpers for RTC chips |

<a id="12--what-your-simulator-provides"></a>
### 1.2. What your simulator provides

- **Bus implementation**: A struct implementing `Bus` with your bus's address width,
  error type, and dispatch logic (e.g., S-100 linear card walk vs. Unibus CSR page).
- **Card/device manager**: Manages inserted cards, handles address conflicts, and
  provides interrupt arbitration specific to your bus (e.g., Z80 daisy chain vs.
  PDP-11 BR priority levels).
- **Machine struct**: Owns the bus, CPU, debugger, and provides the run loop.
- **Configuration loader**: TOML/JSON config that builds a machine from card definitions.
- **CLI/MCP/DAP servers**: User interfaces that delegate to the machine.


<a id="2--the-bus-trait"></a>
## 2. Bus interface

The `Bus` trait is the central interconnect between CPU and peripherals. It is generic
over two associated types:

```rust
pub trait Bus: Send + Sync {
    type Addr: Copy + Debug + Display + Default + Ord + Hash + Send + Sync + 'static;
    type Error: Debug + Send + Sync + 'static;

    fn read(&mut self, addr: Self::Addr) -> Result<u8, Self::Error>;
    fn write(&mut self, addr: Self::Addr, val: u8) -> Result<(), Self::Error>;
    fn io_read(&mut self, addr: Self::Addr) -> Result<u8, Self::Error>;
    fn io_write(&mut self, addr: Self::Addr, val: u8) -> Result<(), Self::Error>;

    // External data memory for split-bus CPUs; defaults delegate to read/write.
    fn ext_data_read(&mut self, addr: Self::Addr) -> Result<u8, Self::Error> {
        self.read(addr)
    }
    fn ext_data_write(&mut self, addr: Self::Addr, val: u8) -> Result<(), Self::Error> {
        self.write(addr, val)
    }

    fn tick(&mut self, t_states: u32);

    fn interrupt_pending(&self) -> bool;
    fn int_acknowledge(&mut self) -> Option<u8>;
    fn reti_notify(&mut self);

    // Watchpoint hooks (defaults are no-ops / None).
    fn set_watchpoints(&mut self, watchpoints: &[(u32, WatchKind)]) {}
    fn clear_watch_hit(&mut self) {}
    fn take_watch_hit(&mut self) -> Option<WatchHit> { None }

    fn bus_master_active(&self) -> bool { false }
    fn tick_bus_masters(&mut self) -> u32 { 0 }
}
```

CPUs that translate a 16-bit logical address to a wider physical address
additionally implement `PhysicalBus`. That trait is deliberately narrow — its
supertrait bound is `Bus<Addr = u16, Error = std::convert::Infallible>`, so it
applies only to 16-bit-addressed, infallible buses. It adds
`mem_read_phys`/`mem_write_phys` over a `u32` physical address, plus default hooks for Z180 interrupt pins (`interrupt_pending_int0`,
`interrupt_pending_int1`, and `interrupt_pending_int2`), CSIO external clock
edges (`take_csio_ext_clock`), and interrupt tracing (`record_int_trace`).
INT0 defaults to the generic pending interrupt; INT1 and INT2 default to false.

<a id="21--address-type"></a>
### 2.1. Address type

`Addr` is the CPU's *logical* address width, not its physical one. The
`PhysicalBus` interface uses `Addr = u16`; a wider physical space is reached
through its physical-access methods rather than by widening `Addr`.

- **Z80/8080/Z180**: `Addr = u16` — 64 KB logical; the Z180 MMU maps it to
  20 bits through `PhysicalBus`
- **8086/8088**: `Addr = u16` — the CPU forms segment:offset itself and
  reaches the 20-bit physical space through `PhysicalBus`
- **MC68000**: `Addr = u16` — the 16-bit S-100 port; 24-bit physical memory
  through `PhysicalBus`

Other buses can use a wider logical address through the `Addr` associated type.

<a id="22--error-type"></a>
### 2.2. Error type

- **Simple buses**: `Error = std::convert::Infallible` — the compiler eliminates all
  `Err` branches at zero cost.
- **Faulting buses**: Use a custom error enum (e.g., `BusTrap { kind, addr }` for
  PDP-11 bus errors, or `M68kFault` for MC68000 address errors).

<a id="23--io-space"></a>
### 2.3. I/O space

The `io_read`/`io_write` methods take `Self::Addr`, the same address type as memory.
This matches the x86/Z80/8051 architectures, where I/O space is separate from memory.
CPUs with only memory-mapped I/O (6502, 6809) leave these methods unused and decode
peripherals in `read`/`write`. The MC68000 is the middle case: it has no I/O
instructions, so `sim-68k` redirects a high physical window (FF0000h–FFFFFFh) onto
`io_read`/`io_write` to reach the 16-bit S-100 port space.

<a id="24--interrupt-protocol"></a>
### 2.4. Interrupt protocol

- `interrupt_pending()` — polls all installed interrupt sources
- `int_acknowledge()` — performs an interrupt-acknowledge cycle, returns vector byte
- `reti_notify()` — called when CPU decodes RETI; walks daisy chain to clear IUS

These are generic enough for Z80 IM2 daisy chains, 8259A PIC cascades, and plain
level-sensitive IRQ lines. Your bus implementation decides the arbitration policy.


<a id="3--the-card-trait"></a>
## 3. Card interface

Cards (or "devices" in DEC terminology) are plug-in peripherals. The `Card<A>` trait is
generic over address type `A`:

Only `name` and `describe` are required; everything else has a default. The
abridged shape below omits `peek_mem`, `card_type_name`, `log_targets`,
`log_target_base`, the `uart_modem_*` / `uart_set_loopback` /
`uart_serial_params` / `uart_display_reset` signals, and
`disk_set_geometry` / `disk_flush` / `disk_flush_all` — see `src/card.rs`
for the full set:

```rust
pub trait Card<A: Copy>: Send + Sync {
    fn name(&self) -> &str;
    fn describe(&self) -> CardInfo;

    // Address decode — return Some to claim, None to pass through
    fn mem_read(&mut self, addr: A) -> Option<u8> { None }
    fn mem_write(&mut self, addr: A, val: u8) -> Option<()> { None }
    fn io_read(&mut self, addr: A) -> Option<u8> { None }
    fn io_write(&mut self, addr: A, val: u8) -> Option<()> { None }

    fn needs_tick(&self) -> bool { false }
    fn tick(&mut self, t_states: u32) {}
    fn reset(&mut self) {}

    // Interrupt support
    fn as_interrupt_source(&self) -> Option<&dyn InterruptSource> { None }
    fn as_interrupt_source_mut(&mut self) -> Option<&mut dyn InterruptSource> { None }

    // Bus mastering (DMA)
    fn as_bus_master(&self) -> Option<&dyn BusMaster> { None }
    fn as_bus_master_mut(&mut self) -> Option<&mut dyn BusMaster> { None }

    // Serial ports
    fn uart_names(&self) -> &[&str] { &[] }
    fn uart_read(&mut self, port: &str) -> Vec<u8> { Vec::new() }
    fn uart_write(&mut self, port: &str, data: &[u8]) {}

    // Disk drives (CardResult<T> = Result<T, CardError>)
    fn disk_list(&self) -> Vec<DiskInfo> { Vec::new() }
    fn disk_attach(&mut self, drive_type: &str, drive_num: usize, path: &str,
                   read_only: bool) -> CardResult<()> { Err(CardError::NotSupported) }
    fn disk_detach(&mut self, drive_type: &str, drive_num: usize) -> CardResult<()> {
        Err(CardError::NotSupported)
    }

    // Configuration
    fn config_summary(&self) -> String { String::new() }
    fn config_options(&self) -> Vec<ConfigOption> { Vec::new() }
    fn save_config(&self) -> Option<serde_json::Value> { None }
    fn apply_config(&mut self, params: &serde_json::Value) -> CardResult<()> {
        Err(CardError::NotSupported)
    }
}
```

<a id="31--address-decode-pattern"></a>
### 3.1. Address decode pattern

Cards return `Some(value)` to claim an address, `None` to pass through. Your bus
implementation walks cards in priority order; the first card to return `Some` wins.

<a id="32--card-helpers"></a>
### 3.2. Card helpers

`retrosim-core` provides reusable helpers to simplify card implementations:

**`MemoryRange`** — base/size address decode, returning the byte offset into a
backing buffer:
```rust
let rom = MemoryRange::new(0xF000, 0x1000); // base, size in bytes

// In Card::mem_read:
self.rom.offset(addr).map(|off| self.rom_data[off])
```

**`IoPortRange`** — I/O port decode with a configurable mask width:
```rust
let ports = IoPortRange::new(0x20, 4, IoAddrDecode::Bits8); // base, count, decode
if let Some(offset) = ports.offset(addr) { /* offset is 0..count */ }
```

**`impl_interrupt_source!`** — generates an `InterruptSource` impl that delegates
to a chip field (the `ieo()` method is derived as `!field.ius()`):
```rust
impl_interrupt_source!(MyCard, ctc,
    pending     = any_channel_interrupting,
    acknowledge = acknowledge,
    reti        = reti,
    ius         = ius_set,
);
```

**`UartTxBuffer`** — Accumulates TX bytes for efficient batch reads.

`card_helpers` also carries free functions for the disk and UART plumbing that
every card repeats: `validate_drive_type`, `validate_drive_num` and
`validate_drive` turn a bad `disk_attach`/`disk_detach` argument into the right
`CardError`, and `resolve_dual_uart` maps a port name (configured or default)
onto channel 0 or 1 of a two-channel UART.


<a id="4--the-chip-trait"></a>
## 4. Chip interface

Chips are IC-level components owned by cards. They never see the bus directly — the
card translates bus operations into chip register reads/writes.

```rust
pub trait Chip: Send + Sync {
    fn name(&self) -> &str;
    fn read(&mut self, addr: u8) -> u8;
    fn write(&mut self, addr: u8, val: u8);
    fn tick(&mut self, t_states: u32);
}
```

`ChipSignals` is a small struct (`interrupt_pending`, `bus_request`) a chip can
report back to its card. The `InterruptSource` trait adds Z80-style daisy-chain
interrupt support:

```rust
pub trait InterruptSource: Send + Sync {
    fn interrupt_pending(&self) -> bool;
    fn acknowledge(&mut self) -> u8;   // sets IUS, returns the vector byte
    fn reti_notify(&mut self);
    fn under_service(&self) -> bool;   // IUS flag
    fn ieo(&self) -> bool;             // Interrupt Enable Out to lower priority
}
```


<a id="5--the-cpu-trait"></a>
## 5. CPU interface

```rust
pub trait Cpu: Send + Sync {
    type Addr: Copy + Debug + Display + Default + Ord + Hash + Send + Sync + 'static;
    type Error: Debug + Send + Sync + 'static;
    type Regs: Registers;
    type Bus: Bus<Addr = Self::Addr, Error = Self::Error>;

    fn step(&mut self, bus: &mut Self::Bus) -> u32;   // executes one instr, returns T-states
    fn registers(&self) -> &Self::Regs;
    fn registers_mut(&mut self) -> &mut Self::Regs;
    fn reset(&mut self);
    fn interrupt(&mut self, bus: &mut Self::Bus);
    fn nmi(&mut self, bus: &mut Self::Bus);
    fn is_halted(&self) -> bool;
    fn cpu_type_name(&self) -> &'static str;
}
```

`Registers` exposes `pc`/`sp`/flags (and optional Z80 register pairs); `RegistersDyn`
adds object-safe name/value enumeration for the debugger and DAP/MCP servers.


<a id="6--debugger"></a>
## 6. Debugger

The `Debugger` struct provides:

- **Breakpoints**: PC-based execution breakpoints
- **Watchpoints**: Memory read/write/access watchpoints with value capture
- **Trace log**: Ring buffer of recent instruction executions (PC + opcode bytes)
- **State management**: Enable/disable tracing, clear logs

The debugger is owned by your machine struct and checked in the run loop:

```rust
loop {
    let pc = cpu.registers().pc();
    if debugger.should_break(pc) {
        return RunResult { stop_reason: StopReason::Break, t_elapsed, pc };
    }
    if debugger.trace_enabled() {
        debugger.record_trace(trace_entry); // a TraceEntry { pc, bytes, t_states, .. }
    }
    let t = cpu.step(&mut bus);
    // ...
}
```


<a id="7--transport-layer"></a>
## 7. Transport layer

`Transport` abstracts character I/O backends (TCP, telnet, serial ports, SSH):

```rust
pub trait Transport: Read + Write + Send {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
    fn description(&self) -> &str;
    fn is_connected(&self) -> bool { true }
    fn is_listening(&self) -> bool { false }
    fn reconfigure(&mut self, params: &SerialParams) -> io::Result<()> { Ok(()) }
    fn modem_control(&mut self) -> Option<&mut dyn ModemControl> { None }
}
```

`ModemControl` adds the RS-232 control lines — `set_dtr`, `set_rts` and
`set_break` outbound, `read_cts` / `read_dsr` / `read_dcd` / `read_ri`
inbound. Direct serial transports operate local RS-232 lines; RFC 2217
transports send settings and modem requests to a remote serial server.
TCP/telnet/SSH implementations in `retrosim-transports`
provide synthetic lines: `set_rts` gates socket reads and CTS/DSR/DCD read as
asserted. The trait's default implementation exposes no modem control.

`reconfigure` applies to direct serial and RFC 2217 backends; the default
implementation is a no-op. `set_read_timeout(None)` blocks indefinitely,
while `Some(Duration::ZERO)` requests polling with `WouldBlock` when no data
is available. A finite nonzero timeout bounds the wait.

`TransportInfo` records the simulator port name, original transport spec,
listen flag, description, and connection state. It has no separate local or
peer address fields; a backend may include those in its description.


<a id="8--serial-filters"></a>
## 8. Serial filters

`SerialFilter` transforms data between guest and host:

```rust
pub trait SerialFilter: Send {
    fn filter_tx(&mut self, input: &[u8]) -> Vec<u8>;
    fn filter_rx(&mut self, input: &[u8]) -> Vec<u8>;
    fn name(&self) -> &str;
    fn reset(&mut self);
    fn pending_loopback(&mut self) -> Option<Vec<u8>> { None }
}
```

`FilterPipeline` chains filters, applying them in order for TX (guest→host) and
in reverse for RX (host→guest), and returns `Cow::Borrowed` (zero-copy) when the
pipeline is empty. Concrete filters (7-bit strip, terminal translation, loopback,
etc.) live in the consuming crates, not in this foundation.


<a id="85--performance-instrumentation-perf-module"></a>
## 8.5. Performance instrumentation

Two primitives account for wall time and simulated time without display
dependencies. Convenience methods sample `std::time::Instant`; the timing
arithmetic accepts injected clock values internally for deterministic tests.

```rust
pub struct PerfTracker { /* internal */ }
impl PerfTracker {
    pub fn new(sim_hz: u32) -> Self;
    pub fn begin_frame() -> std::time::Instant;           // alias for Instant::now()
    pub fn end_frame(&mut self, wall_start: Instant, sim_cycles: u64)
        -> Option<PerfSnapshot>;                          // emits once per wall-second
    pub fn status(&self) -> Option<&str>;                 // "speed=1.02x, cpu=97%, fps=60"
    pub fn snapshot(&self) -> Option<PerfSnapshot>;       // raw metrics
}

pub struct WallProportionalBudget { /* internal */ }
impl WallProportionalBudget {
    pub fn new(sim_hz: u32, frame_cycles: u64) -> Self;   // clamps [1, 8] frames
    pub fn with_bounds(sim_hz, frame_cycles, min_frames, max_frames) -> Self;
    pub fn budget(&mut self, wall_now: Instant) -> u64;   // sim cycles for this call
}
```

**`PerfTracker`** accumulates wall-time, sim-cycles, and frame count across
CPU-driving calls and emits a `speed=… cpu=… fps=…` status once per wall-second
(`speed = sim_s / wall_s`, both measured only over the active-CPU portion).

**`WallProportionalBudget`** decouples sim rate from render rate: when the runner
drives the CPU at less than 60 Hz, it sizes each call's sim-cycle budget from the
elapsed wall time so sim-time tracks wall-time 1:1. The budget is clamped to
`[1, 8]` frames — the floor keeps interaction snappy, the ceiling stops a long
pause (debugger stop, laptop sleep) from triggering a multi-minute catch-up.


<a id="9--execution-control"></a>
## 9. Execution control

```rust
pub enum StopReason {
    Halt,                   // CPU executed HALT
    Break,                  // breakpoint hit
    Watchpoint(WatchHit),   // watchpoint triggered
    Canceled,              // CancellationToken fired
    TStateLimit,            // T-state budget exhausted
    IoTrap,                 // I/O trap (e.g. CP/M BDOS trap)
}

// RunResult { stop_reason, t_elapsed, pc }
pub struct CancellationToken { /* Arc<AtomicBool>, cloneable */ }
```


<a id="10--building-a-new-simulator"></a>
## 10. Building a new simulator

### Step 1: Define your bus-specific core crate

```toml
# my-core/Cargo.toml
[dependencies]
retrosim-core = { path = "../retrosim-core" }
```

Implement:
- Your bus struct implementing `retrosim_core::bus::Bus`
- Your device/card manager
- Your interrupt arbitration logic

### Step 2: Implement or reuse CPU crates

CPU crates (sim-z80, sim-pdp11, etc.) depend on retrosim-core and your bus-core crate.
They implement `retrosim_core::cpu::Cpu` with your bus as the associated type.

### Step 3: Implement peripheral cards/devices

Each peripheral implements `retrosim_core::card::Card<A>` with your bus's address type.
Use `MemoryRange`, `IoPortRange`, and `impl_interrupt_source!` from `card_helpers`.

### Step 4: Build your machine

Your machine struct owns the bus, CPU, debugger, and provides the run loop. Use
`StopReason`, `RunResult`, and `CancellationToken` from `retrosim_core::machine`.

### Step 5: Add user interfaces

MCP, DAP, and CLI servers interact with your machine through the trait interfaces.
The register, breakpoint, and disassembly APIs are all defined in retrosim-core.


<a id="11--api-conventions"></a>
## 11. API conventions

1. **All public items have doc comments.** `#![deny(missing_docs)]` is enforced.
2. **Follow each trait’s thread bounds.** `Bus`, `Card`, `Chip`, and `Cpu`
   require `Send + Sync`; `Transport` and `SerialFilter` require `Send` only.
   Do not assume every trait object supports concurrent shared access.
3. **Use typed errors.** `CardError` represents card-operation failures;
   `TransportError` represents transport failures. Preserve underlying I/O error sources.
4. **Logging uses `tracing`.** Not `log`. Use `tracing::{debug, info, warn, error}`.
5. **Serialization uses `serde`.** CardInfo, ConfigOption, DiskInfo all derive Serialize.
6. **No `unsafe` code.** The entire crate is safe Rust.
7. **Macro paths use `$crate::`.**  Re-export with `pub use retrosim_core::impl_interrupt_source;`.


<a id="12--module-reference"></a>
## 12. Verification

Unit tests cover bus adapters, card helpers, CPU helpers, debugging, serial
filters, transport defaults, cancellation, calendar conversion, and performance
accounting. Calendar fixtures exercise epoch, leap-year, rollover, and distant-year boundaries;
they are synthetic test inputs rather than development timestamps.

Tests must not depend on wall-clock scheduling. Inject clock values for timing
logic, as the `perf` tests do, so coverage instrumentation does not change the
outcome. Every module with behavior should have unit tests. Trait changes require
coordinated updates in consumers. Keep bus-specific types in those consumers.

See the [README verification commands](../README.md#verification) for the local
gates and coverage workflow. Generated Rustdoc is the complete API reference;
the examples here are abridged integration sketches.
