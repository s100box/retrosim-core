// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Bus-agnostic machine types.
//!
//! Contains [`StopReason`], [`RunResult`], and [`CancellationToken`] which
//! are shared across all bus architectures.  The S-100-specific `Machine<C>`,
//! `MachineOps`, and `MultiCpuMachine` live in `sim-core`.

use crate::debugger::WatchHit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Reason the machine stopped executing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// CPU executed a HALT instruction.
    Halt,
    /// A breakpoint was hit.
    Break,
    /// A watchpoint was triggered.
    Watchpoint(WatchHit),
    /// Execution was canceled by the user or MCP client.
    Canceled,
    /// T-state budget exhausted.
    TStateLimit,
    /// An I/O trap was triggered (e.g., CP/M BDOS trap for ZEXALL).
    IoTrap,
}

/// Result of a `run_until` call.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Why execution stopped.
    pub stop_reason: StopReason,
    /// Total T-states elapsed during this run.
    pub t_elapsed: u64,
    /// Program counter at the point execution stopped.
    pub pc: u32,
}

/// A cooperative cancellation token for aborting `run_until` mid-execution.
///
/// Wraps an `Arc<AtomicBool>`. Clone the token and call `cancel()` from
/// another thread or async task to set the flag. The `run_until` loop
/// checks the flag periodically and returns `StopReason::Canceled`.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    canceled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a new cancellation token (not canceled).
    pub fn new() -> Self {
        Self {
            canceled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal cancellation. Any `run_until` checking this token will stop.
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.canceled.load(Ordering::Acquire)
    }

    /// Reset the token so it can be reused for another run.
    pub fn reset(&self) {
        self.canceled.store(false, Ordering::Release);
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reason_equality() {
        assert_eq!(StopReason::Halt, StopReason::Halt);
        assert_ne!(StopReason::Halt, StopReason::Break);
    }

    #[test]
    fn cancellation_token_basic() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
        token.reset();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancellation_token_clone_shares_state() {
        let token1 = CancellationToken::new();
        let token2 = token1.clone();
        assert!(!token2.is_cancelled());
        token1.cancel();
        assert!(token2.is_cancelled());
    }

    #[test]
    fn cancellation_token_default() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
    }
}
