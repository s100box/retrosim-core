// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Disassembler trait for CPU instruction decoding.

/// Result of disassembling a single instruction.
#[derive(Debug, Clone)]
pub struct DisasmResult {
    /// Assembly mnemonic (e.g., "LD A, (HL)").
    pub mnemonic: String,
    /// Number of bytes consumed by this instruction.
    pub bytes_consumed: u8,
    /// Optional annotation (e.g., resolved symbol name or address).
    pub annotation: Option<String>,
    /// T-state timing as `(min, max)`. For unconditional instructions
    /// min == max. For conditional instructions (e.g., JR NZ), min is the
    /// not-taken time and max is the taken time. `None` if not available.
    pub t_states: Option<(u8, u8)>,
}

/// Trait for CPU-specific disassemblers.
///
/// Each CPU architecture provides its own implementation that decodes
/// raw bytes into human-readable assembly mnemonics.
pub trait Disassembler: Send + Sync {
    /// Disassemble one instruction starting at `addr`.
    ///
    /// `bytes` must hold enough bytes for the implementation's longest
    /// supported instruction, including prefixes and operands.
    /// Returns the mnemonic, number of bytes consumed, and an optional
    /// annotation.
    fn disassemble(&self, addr: u32, bytes: &[u8]) -> DisasmResult;
}

/// Disassemble `count` instructions starting at `addr`.
///
/// Uses `read_fn` to fetch bytes from memory. Returns a list of
/// `(address, DisasmResult)` pairs and the address after the last instruction.
/// This convenience helper supplies an eight-byte decode window; callers for
/// longer encodings must call [`Disassembler::disassemble`] with a larger buffer.
/// `read_fn` should peek without causing device side effects.
pub fn disassemble_n(
    disasm: &dyn Disassembler,
    start: u32,
    count: usize,
    mut read_fn: impl FnMut(u32, &mut [u8]),
) -> (Vec<(u32, DisasmResult)>, u32) {
    let mut results = Vec::with_capacity(count);
    let mut addr = start;
    for _ in 0..count {
        let mut buf = [0u8; 8];
        read_fn(addr, &mut buf);
        let result = disasm.disassemble(addr, &buf);
        let consumed = result.bytes_consumed as u32;
        results.push((addr, result));
        addr = addr.wrapping_add(consumed);
    }
    (results, addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disasm_result_fields() {
        let r = DisasmResult {
            mnemonic: "NOP".into(),
            bytes_consumed: 1,
            annotation: None,
            t_states: Some((4, 4)),
        };
        assert_eq!(r.bytes_consumed, 1);
        assert_eq!(r.mnemonic, "NOP");
        assert!(r.annotation.is_none());
    }

    #[test]
    fn disassembler_is_object_safe() {
        let _: Option<Box<dyn Disassembler>> = None;
    }

    /// Stub disassembler for testing `disassemble_n`.
    struct StubDisasm;
    impl Disassembler for StubDisasm {
        fn disassemble(&self, addr: u32, bytes: &[u8]) -> DisasmResult {
            let consumed = if bytes[0] == 0xCB { 2 } else { 1 };
            DisasmResult {
                mnemonic: format!("STUB@{:#06X}", addr),
                bytes_consumed: consumed,
                annotation: None,
                t_states: None,
            }
        }
    }

    #[test]
    fn disassemble_n_basic() {
        let mem = [0x00u8, 0xCB, 0x01, 0x00]; // NOP, CB 01, NOP
        let (results, end_addr) = disassemble_n(&StubDisasm, 0, 3, |addr, buf| {
            for (i, b) in buf.iter_mut().enumerate() {
                let idx = (addr as usize + i) % mem.len();
                *b = mem[idx];
            }
        });
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 0); // addr of first instr
        assert_eq!(results[0].1.bytes_consumed, 1);
        assert_eq!(results[1].0, 1); // addr of second instr
        assert_eq!(results[1].1.bytes_consumed, 2); // CB prefix
        assert_eq!(results[2].0, 3); // addr of third instr
        assert_eq!(end_addr, 4);
    }
}
