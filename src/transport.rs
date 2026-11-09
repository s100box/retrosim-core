// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Transport trait and types for character I/O.
//!
//! A `Transport` is a bidirectional byte stream connecting a simulated
//! serial channel (DART, SIO, UART, PIO) to an external I/O source:
//! host serial port, TCP/telnet/UDP socket, or the local terminal.

use std::io::{self, Read, Write};
use std::time::Duration;

/// A bidirectional byte-stream transport for character I/O.
///
/// Extends `Read + Write + Send` with timeout control and metadata.
/// All concrete transports (serial, TCP, telnet, UDP) implement this
/// trait.  The simulation run loop interacts with transports only
/// through `Box<dyn Transport>`.
pub trait Transport: Read + Write + Send {
    /// Set the read timeout.
    ///
    /// `None` means block indefinitely.  `Some(Duration::ZERO)` means
    /// non-blocking (return `WouldBlock` immediately if no data).
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;

    /// Human-readable description of this transport.
    ///
    /// Examples: `"serial:/dev/ttyUSB0@9600"`, `"tcp:192.168.1.5:2023"`,
    /// `"telnet-server:0.0.0.0:23 (peer 10.0.0.1:54321)"`.
    fn description(&self) -> &str;

    /// Whether the transport is currently connected.
    ///
    /// Reports connection state tracked by the implementation, rather than
    /// actively probing the peer. The default is `true`; implementations
    /// using that default (including raw TCP, direct serial and UDP) do not
    /// detect disconnection through this method. Callers must still handle
    /// EOF and I/O errors.
    fn is_connected(&self) -> bool {
        true
    }

    /// Whether this transport is a "listening" server that will accept
    /// reconnects from new clients over its lifetime.
    ///
    /// Returns `true` for a transport that retains a listener and can accept
    /// another client, such as `TelnetTransport::accept`. Raw
    /// `TcpTransport::accept` accepts one peer and drops its listener, so it
    /// uses the default `false`.
    ///
    /// A consumer's read loop can use this flag to decide whether a client
    /// disconnect is recoverable: listening
    /// transports survive it (the listener is still bound, just waiting
    /// for the next client), while client-mode transports exit cleanly
    /// because the far end went away for good.
    ///
    /// Default: `false` (client-mode).
    fn is_listening(&self) -> bool {
        false
    }

    /// Reconfigure the serial line parameters.
    ///
    /// Direct serial and RFC 2217 transports apply these settings to a
    /// physical or remote serial port. Other transports may ignore the call
    /// (the default is a no-op).
    fn reconfigure(&mut self, _params: &crate::serial::SerialParams) -> io::Result<()> {
        Ok(())
    }

    /// Return a mutable reference to modem control lines, if supported.
    ///
    /// `SerialTransport` returns `Some` with real RS-232 line driver
    /// behavior; RFC 2217 transports carry requests and status over the
    /// network to a remote serial device. Other network transports
    /// (SSH, TCP, telnet) also return
    /// `Some`, but theirs is a **synthetic** implementation that
    /// routes RTS into a software read-gate and answers CTS/DSR/DCD
    /// with "asserted" (the wire has no sideband to carry them).
    /// Transports with no modem-control modeling at all return
    /// `None`.
    fn modem_control(&mut self) -> Option<&mut dyn ModemControl> {
        None
    }
}

/// RS-232 modem control lines.
///
/// Direct serial transports operate the local device; RFC 2217 transports
/// operate a remote device through its server. Synthetic implementations
/// leave inputs asserted (CTS/DSR/DCD = true, RI = false) so software that
/// polls modem status can run without physical sideband lines.
pub trait ModemControl {
    /// Set Data Terminal Ready (output).
    fn set_dtr(&mut self, active: bool) -> io::Result<()>;
    /// Set Request To Send (output).
    fn set_rts(&mut self, active: bool) -> io::Result<()>;
    /// Set break condition on the TX line.
    ///
    /// When `active` is true, the TX line is held low (spacing).
    /// When false, normal transmission resumes.
    fn set_break(&mut self, active: bool) -> io::Result<()>;
    /// Read Clear To Send (input).
    fn read_cts(&mut self) -> io::Result<bool>;
    /// Read Data Set Ready (input).
    fn read_dsr(&mut self) -> io::Result<bool>;
    /// Read Data Carrier Detect (input).
    fn read_dcd(&mut self) -> io::Result<bool>;
    /// Read Ring Indicator (input).
    fn read_ri(&mut self) -> io::Result<bool>;
}

/// Errors from transport operations.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// I/O error from the underlying transport.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// Invalid transport spec string.
    #[error("invalid transport spec: {0}")]
    InvalidSpec(String),
    /// Transport-specific error (e.g., serial port open failure).
    #[error("transport error: {0}")]
    Transport(String),
    /// The specified port name was not found.
    #[error("port not found: {0}")]
    PortNotFound(String),
}

/// Information about a transport binding (for display).
#[derive(Debug, Clone)]
pub struct TransportInfo {
    /// Console port name (e.g., "SIO-A").
    pub port_name: String,
    /// Transport spec string used to create this binding.
    pub spec: String,
    /// Whether this is a server (listen) binding.
    pub listen: bool,
    /// Human-readable description from the transport.
    pub description: String,
    /// Whether the transport is currently connected.
    pub connected: bool,
}

/// Mock transport for testing.
///
/// Stores written bytes in `tx` and serves pre-loaded bytes from `rx`.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;

    /// A mock transport backed by in-memory queues.
    /// Reads are always nonblocking; timeout configuration is ignored.
    pub struct MockTransport {
        /// Bytes available to read (pre-loaded by test).
        pub rx: VecDeque<u8>,
        /// Bytes written by the system under test.
        pub tx: Vec<u8>,
        /// Description string.
        pub desc: String,
    }

    impl MockTransport {
        /// Create a new empty mock transport.
        pub fn new(desc: &str) -> Self {
            Self {
                rx: VecDeque::new(),
                tx: Vec::new(),
                desc: desc.to_string(),
            }
        }

        /// Pre-load bytes for reading.
        pub fn feed(&mut self, data: &[u8]) {
            self.rx.extend(data);
        }
    }

    impl Read for MockTransport {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            let n = buf.len().min(self.rx.len());
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "no data"));
            }
            for (slot, byte) in buf.iter_mut().zip(self.rx.drain(..n)) {
                *slot = byte;
            }
            Ok(n)
        }
    }

    impl Write for MockTransport {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.tx.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transport for MockTransport {
        fn set_read_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
            Ok(())
        }

        fn description(&self) -> &str {
            &self.desc
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_error_display() {
        let e = TransportError::InvalidSpec("bad:spec".into());
        assert!(e.to_string().contains("bad:spec"));

        let e = TransportError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "broken"));
        assert!(e.to_string().contains("broken"));

        let e = TransportError::PortNotFound("SIO-Z".into());
        assert!(e.to_string().contains("SIO-Z"));
    }

    #[test]
    fn mock_transport_read_write() {
        use std::io::{Read, Write};
        let mut t = mock::MockTransport::new("test");
        t.feed(b"hello");
        let mut buf = [0u8; 5];
        let n = t.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello");

        t.write_all(b"world").unwrap();
        assert_eq!(t.tx, b"world");
    }

    #[test]
    fn mock_transport_empty_read() {
        use std::io::Read;
        let mut t = mock::MockTransport::new("test");
        let mut buf = [0u8; 1];
        let err = t.read(&mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn zero_length_mock_reads_preserve_queued_input() {
        let mut transport = mock::MockTransport::new("test");
        assert_eq!(transport.read(&mut []).unwrap(), 0);
        transport.feed(b"queued");
        assert_eq!(transport.read(&mut []).unwrap(), 0);
        assert_eq!(transport.rx.len(), 6);
        let mut first = [0; 2];
        assert_eq!(transport.read(&mut first).unwrap(), 2);
        assert_eq!(&first, b"qu");
        assert_eq!(transport.rx.iter().copied().collect::<Vec<_>>(), b"eued");
    }

    #[test]
    fn mock_transport_description() {
        let t = mock::MockTransport::new("mock:test");
        assert_eq!(t.description(), "mock:test");
    }

    #[test]
    fn mock_transport_default_modem_control() {
        let mut t = mock::MockTransport::new("test");
        assert!(t.modem_control().is_none());
    }

    #[test]
    fn mock_transport_default_connected() {
        let t = mock::MockTransport::new("test");
        assert!(t.is_connected());
    }

    #[test]
    fn mock_transport_default_trait_methods() {
        use std::io::Write;
        let mut t = mock::MockTransport::new("test");

        // Default is_listening is false (client-mode).
        assert!(!t.is_listening());

        // Default reconfigure is a no-op that succeeds.
        let params = crate::serial::SerialParams {
            baud_rate: 9600,
            data_bits: 8,
            parity: crate::serial::SerialParity::None,
            stop_bits: crate::serial::SerialStopBits::One,
        };
        assert!(t.reconfigure(&params).is_ok());

        // MockTransport's set_read_timeout and flush both succeed.
        assert!(t
            .set_read_timeout(Some(std::time::Duration::from_millis(10)))
            .is_ok());
        assert!(t.flush().is_ok());
    }
}
