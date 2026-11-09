// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Serial communication types shared across bus architectures.
//!
//! Contains modem signal definitions, serial line parameters, and
//! related types used by UART cards and transport layers.

/// Guest UART modem output signals (driven by the guest CPU).
///
/// These are signals the guest UART drives onto the serial line:
/// DTR (Data Terminal Ready), RTS (Request To Send), and a break
/// condition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModemOutputs {
    /// Data Terminal Ready — guest asserts when ready to communicate.
    pub dtr: bool,
    /// Request To Send — guest asserts when ready to transmit.
    pub rts: bool,
    /// Send Break — guest is transmitting a break condition (TxD held low).
    pub sending_break: bool,
}

/// Guest UART modem input signals (what the guest UART sees on its pins).
///
/// These are signals the remote end drives to the guest UART:
/// CTS, DSR, DCD, and RI. The [`Default`] impl asserts CTS/DSR/DCD
/// for virtual terminal operation (so guest software doesn't hang
/// waiting for modem handshake).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModemInputs {
    /// Clear To Send — remote is ready to receive.
    pub cts: bool,
    /// Data Set Ready — remote modem is ready.
    pub dsr: bool,
    /// Data Carrier Detect — carrier signal is present.
    pub dcd: bool,
    /// Ring Indicator — incoming call detected.
    pub ri: bool,
}

impl Default for ModemInputs {
    /// Virtual terminal defaults: handshake signals asserted so guest
    /// software doesn't hang waiting for modem readiness.
    fn default() -> Self {
        Self {
            cts: true,
            dsr: true,
            dcd: true,
            ri: false,
        }
    }
}

/// Serial line parameters as configured by the guest UART.
///
/// Returned by `Card::uart_serial_params()` to let the transport
/// layer reconfigure the host serial port when the guest changes settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialParams {
    /// Baud rate in bits per second.
    pub baud_rate: u32,
    /// Number of data bits (5, 6, 7, or 8).
    pub data_bits: u8,
    /// Parity mode.
    pub parity: SerialParity,
    /// Number of stop bits.
    pub stop_bits: SerialStopBits,
}

/// Parity mode for serial communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerialParity {
    /// No parity bit.
    #[default]
    None,
    /// Even parity.
    Even,
    /// Odd parity.
    Odd,
}

/// Number of stop bits for serial communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerialStopBits {
    /// One stop bit.
    #[default]
    One,
    /// One and a half stop bits.
    OnePointFive,
    /// Two stop bits.
    Two,
}

impl ModemOutputs {
    /// Pack into the low 2 bits of a control byte (RTAC Serial format).
    ///
    /// Bit 0 = DTR, bit 1 = RTS.
    pub fn to_control_bits(self) -> u8 {
        let mut v = 0u8;
        if self.dtr {
            v |= 0x01;
        }
        if self.rts {
            v |= 0x02;
        }
        v
    }
}

impl ModemInputs {
    /// Pack into bits 2–5 of a control byte (RTAC Serial format).
    ///
    /// Bit 2 = CTS, bit 3 = DSR, bit 4 = DCD, bit 5 = RI.
    pub fn to_control_bits(self) -> u8 {
        let mut v = 0u8;
        if self.cts {
            v |= 0x04;
        }
        if self.dsr {
            v |= 0x08;
        }
        if self.dcd {
            v |= 0x10;
        }
        if self.ri {
            v |= 0x20;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modem_outputs_default_is_deasserted() {
        let out = ModemOutputs::default();
        assert!(!out.dtr);
        assert!(!out.rts);
        assert!(!out.sending_break);
        assert_eq!(out.to_control_bits(), 0);
    }

    #[test]
    fn modem_outputs_control_bits() {
        assert_eq!(
            ModemOutputs {
                dtr: true,
                rts: false,
                sending_break: false,
            }
            .to_control_bits(),
            0x01
        );
        assert_eq!(
            ModemOutputs {
                dtr: false,
                rts: true,
                sending_break: false,
            }
            .to_control_bits(),
            0x02
        );
        assert_eq!(
            ModemOutputs {
                dtr: true,
                rts: true,
                sending_break: true,
            }
            .to_control_bits(),
            0x03
        );
    }

    #[test]
    fn modem_inputs_default_asserts_handshake() {
        let inp = ModemInputs::default();
        assert!(inp.cts && inp.dsr && inp.dcd);
        assert!(!inp.ri);
        // CTS(0x04) | DSR(0x08) | DCD(0x10) = 0x1C, RI clear.
        assert_eq!(inp.to_control_bits(), 0x1C);
    }

    #[test]
    fn modem_inputs_control_bits() {
        assert_eq!(
            ModemInputs {
                cts: true,
                dsr: false,
                dcd: false,
                ri: false,
            }
            .to_control_bits(),
            0x04
        );
        assert_eq!(
            ModemInputs {
                cts: false,
                dsr: false,
                dcd: false,
                ri: true,
            }
            .to_control_bits(),
            0x20
        );
        assert_eq!(
            ModemInputs {
                cts: true,
                dsr: true,
                dcd: true,
                ri: true,
            }
            .to_control_bits(),
            0x3C
        );
    }

    #[test]
    fn serial_param_defaults() {
        assert_eq!(SerialParity::default(), SerialParity::None);
        assert_eq!(SerialStopBits::default(), SerialStopBits::One);
    }

    #[test]
    fn serial_params_is_copy_and_eq() {
        let p = SerialParams {
            baud_rate: 9600,
            data_bits: 8,
            parity: SerialParity::Even,
            stop_bits: SerialStopBits::Two,
        };
        let q = p;
        assert_eq!(p, q);
    }
}
