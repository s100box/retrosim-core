// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! CPU trait abstractions.
//!
//! Contains the generic CPU traits shared across all bus architectures.
//! The S-100-specific `CpuDyn` trait and its blanket implementation live
//! in `sim-core` because they depend on `SystemBus`.

use crate::bus::Bus;
use std::fmt::{Debug, Display};
use std::hash::Hash;

// ---------------------------------------------------------------------------
// Shared opcode-table infrastructure
// ---------------------------------------------------------------------------

/// T-state timing for a CPU instruction.
///
/// Used by opcode tables in all CPU crates to record how many T-states
/// (clock cycles) an instruction consumes.
#[derive(Debug, Clone, Copy)]
pub enum TStates {
    /// Instruction always takes a fixed number of T-states.
    Fixed(u8),
    /// Conditional instruction: different timing when branch is taken vs. not taken.
    Conditional {
        /// T-states when the condition is met (branch taken).
        taken: u8,
        /// T-states when the condition is not met (branch not taken).
        not_taken: u8,
    },
}

impl TStates {
    /// Resolve to the actual T-state count given whether a branch was taken.
    #[inline]
    pub fn compute(self, branch_taken: bool) -> u32 {
        match self {
            TStates::Fixed(n) => u32::from(n),
            TStates::Conditional { taken, not_taken } => {
                if branch_taken {
                    u32::from(taken)
                } else {
                    u32::from(not_taken)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Condition code evaluation (shared by Z80 and 8080)
// ---------------------------------------------------------------------------

/// Provides access to the four condition-code flags shared by 8080-family CPUs.
///
/// Both Z80 and 8080 encode condition codes identically:
/// `NZ=0, Z=1, NC=2, C=3, PO=4, PE=5, P=6, M=7`.
/// This trait exposes the four underlying flag bits so that
/// [`eval_condition`] can be CPU-agnostic.
pub trait ConditionFlags {
    /// Zero flag.
    fn zero(&self) -> bool;
    /// Carry flag.
    fn carry(&self) -> bool;
    /// Parity (or parity/overflow) flag bit value.
    fn parity(&self) -> bool;
    /// Sign flag.
    fn sign(&self) -> bool;
}

/// Evaluate an 8080/Z80 condition code against the given flags.
///
/// The 3-bit `cc` field is encoded identically on both CPUs:
///
/// | cc | Condition | Flag test       |
/// |----|-----------|-----------------|
/// | 0  | NZ        | !zero           |
/// | 1  | Z         | zero            |
/// | 2  | NC        | !carry          |
/// | 3  | C         | carry           |
/// | 4  | PO        | !parity         |
/// | 5  | PE        | parity          |
/// | 6  | P         | !sign           |
/// | 7  | M         | sign            |
///
/// # Panics
///
/// Panics if `cc` is outside the three-bit range `0..=7`.
#[inline]
pub fn eval_condition(flags: &impl ConditionFlags, cc: u8) -> bool {
    match cc {
        0 => !flags.zero(),
        1 => flags.zero(),
        2 => !flags.carry(),
        3 => flags.carry(),
        4 => !flags.parity(),
        5 => flags.parity(),
        6 => !flags.sign(),
        7 => flags.sign(),
        _ => unreachable!("invalid condition code: {cc}"),
    }
}

// ---------------------------------------------------------------------------
// Register traits
// ---------------------------------------------------------------------------

/// Common register interface for all CPU types.
///
/// Uses `u32` as the universal representation for program counter and
/// stack pointer values, accommodating all supported architectures
/// (Z80 16-bit through 68k 32-bit).
pub trait Registers: Debug + Send + Sync {
    /// Program counter value.
    fn pc(&self) -> u32;

    /// Stack pointer value.
    fn sp(&self) -> u32;

    /// Set the program counter.
    fn set_pc(&mut self, val: u32);

    /// Set the stack pointer.
    fn set_sp(&mut self, val: u32);

    /// Human-readable summary of CPU flags (e.g., "SZ-H-PNC").
    fn flags_summary(&self) -> String;

    // Optional named-register accessors used by tracing infrastructure
    // ([`crate::bus_context`]). Default impls return 0 so non-Z80 CPUs
    // don't need to override. Z80 (and any other CPU with these register
    // pairs) overrides them to publish the live values.

    /// HL register pair (Z80) or equivalent. Default: 0.
    fn hl(&self) -> u32 {
        0
    }

    /// DE register pair (Z80) or equivalent. Default: 0.
    fn de(&self) -> u32 {
        0
    }

    /// BC register pair (Z80) or equivalent. Default: 0.
    fn bc(&self) -> u32 {
        0
    }

    /// IY index register (Z80). Default: 0.
    fn iy(&self) -> u32 {
        0
    }
}

/// Extended register interface with enumeration support.
///
/// Provides access to all CPU registers as name-value pairs,
/// used by the DAP server and MCP tools for register inspection.
/// Object-safe for dynamic dispatch.
pub trait RegistersDyn: Registers {
    /// Return all registers as `(name, value)` pairs.
    fn all_registers(&self) -> Vec<(String, u64)>;

    /// Set a register by name (case-insensitive).
    ///
    /// Returns `true` if the register was found and set, `false` otherwise.
    /// Names should match those returned by [`all_registers`](Self::all_registers).
    fn set_register(&mut self, name: &str, value: u64) -> bool;

    /// Return a human-readable multi-line register dump.
    ///
    /// Used by the MCP and DAP servers for register status display.
    fn format_registers(&self) -> String;
}

/// Generic CPU trait with static dispatch.
///
/// Uses associated types for address width, error type, register set,
/// and bus type. This enables static dispatch in the performance-critical
/// step loop.
pub trait Cpu: Send + Sync {
    /// Address type (e.g., `u16` for Z80, `u32` for 68k).
    type Addr: Copy + Debug + Display + Default + Ord + Hash + Send + Sync + 'static;

    /// Bus error type (e.g., `std::convert::Infallible` for Z80).
    type Error: Debug + Send + Sync + 'static;

    /// Register set type.
    type Regs: Registers;

    /// Bus type. Must match the address and error types.
    type Bus: Bus<Addr = Self::Addr, Error = Self::Error>;

    /// Execute one instruction and return the number of T-states consumed.
    fn step(&mut self, bus: &mut Self::Bus) -> u32;

    /// Immutable access to the register set.
    fn registers(&self) -> &Self::Regs;

    /// Mutable access to the register set.
    fn registers_mut(&mut self) -> &mut Self::Regs;

    /// Reset the CPU to its power-on state.
    fn reset(&mut self);

    /// Service a maskable interrupt via the bus.
    fn interrupt(&mut self, bus: &mut Self::Bus);

    /// Service a non-maskable interrupt.
    fn nmi(&mut self, bus: &mut Self::Bus);

    /// Returns `true` if the CPU is in a halted state.
    fn is_halted(&self) -> bool;

    /// Returns the CPU type name (e.g. "z80", "z180").
    fn cpu_type_name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_dyn_is_object_safe() {
        let _: Option<Box<dyn RegistersDyn>> = None;
    }

    #[test]
    fn registers_is_object_safe() {
        let _: Option<Box<dyn Registers>> = None;
    }

    // -- TStates tests --

    #[test]
    fn t_states_fixed() {
        let t = TStates::Fixed(4);
        assert_eq!(t.compute(false), 4);
        assert_eq!(t.compute(true), 4);
    }

    #[test]
    fn t_states_conditional() {
        let t = TStates::Conditional {
            taken: 12,
            not_taken: 7,
        };
        assert_eq!(t.compute(true), 12);
        assert_eq!(t.compute(false), 7);
    }

    // -- eval_condition tests --

    struct TestFlags {
        zero: bool,
        carry: bool,
        parity: bool,
        sign: bool,
    }

    impl ConditionFlags for TestFlags {
        fn zero(&self) -> bool {
            self.zero
        }
        fn carry(&self) -> bool {
            self.carry
        }
        fn parity(&self) -> bool {
            self.parity
        }
        fn sign(&self) -> bool {
            self.sign
        }
    }

    #[test]
    fn eval_condition_all_codes() {
        let f = TestFlags {
            zero: true,
            carry: false,
            parity: true,
            sign: false,
        };
        assert!(!eval_condition(&f, 0)); // NZ: !zero = false
        assert!(eval_condition(&f, 1)); //  Z: zero = true
        assert!(eval_condition(&f, 2)); // NC: !carry = true
        assert!(!eval_condition(&f, 3)); //  C: carry = false
        assert!(!eval_condition(&f, 4)); // PO: !parity = false
        assert!(eval_condition(&f, 5)); // PE: parity = true
        assert!(eval_condition(&f, 6)); //  P: !sign = true
        assert!(!eval_condition(&f, 7)); //  M: sign = false
    }

    #[test]
    #[should_panic(expected = "invalid condition code")]
    fn eval_condition_rejects_out_of_range() {
        let f = TestFlags {
            zero: false,
            carry: false,
            parity: false,
            sign: false,
        };
        let _ = eval_condition(&f, 8);
    }

    /// Minimal register set that does not override the optional Z80
    /// register-pair accessors, so it exercises their default `0` impls.
    #[derive(Debug)]
    struct MinimalRegs {
        pc: u32,
        sp: u32,
    }

    impl Registers for MinimalRegs {
        fn pc(&self) -> u32 {
            self.pc
        }
        fn sp(&self) -> u32 {
            self.sp
        }
        fn set_pc(&mut self, val: u32) {
            self.pc = val;
        }
        fn set_sp(&mut self, val: u32) {
            self.sp = val;
        }
        fn flags_summary(&self) -> String {
            "------".to_string()
        }
    }

    #[test]
    fn registers_pc_sp_round_trip() {
        let mut r = MinimalRegs { pc: 0, sp: 0 };
        r.set_pc(0x1234);
        r.set_sp(0xFFFE);
        assert_eq!(r.pc(), 0x1234);
        assert_eq!(r.sp(), 0xFFFE);
        assert_eq!(r.flags_summary(), "------");
    }

    #[test]
    fn register_pair_accessors_default_to_zero() {
        let r = MinimalRegs { pc: 1, sp: 2 };
        assert_eq!(r.hl(), 0);
        assert_eq!(r.de(), 0);
        assert_eq!(r.bc(), 0);
        assert_eq!(r.iy(), 0);
    }

    #[test]
    fn t_states_is_copy() {
        let t = TStates::Fixed(5);
        let u = t; // Copy
        assert_eq!(t.compute(false), u.compute(false));
    }
}
