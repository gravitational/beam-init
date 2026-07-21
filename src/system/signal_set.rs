// Based on sudo-rs code which is
// Copyright (c) 2022-2026 Trifecta Tech Foundation and contributors
// SPDX-License-Identifier: Apache-2.0
// this has been changed to add as_ptr, use pthread_sigmask instead of
// sigprocmask and to remove full and unblock.

use super::cerr;

use std::ffi::c_int;
use std::io;
use std::mem::MaybeUninit;

// A signal set that can be used to mask signals.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub(crate) struct SignalSet {
    raw: libc::sigset_t,
}

impl SignalSet {
    /// Create an empty set.
    pub(crate) fn empty() -> io::Result<Self> {
        let mut set = MaybeUninit::<Self>::zeroed();

        // SAFETY: we pass a valid mutable pointer to `sigemptyset`
        cerr(unsafe { libc::sigemptyset(set.as_mut_ptr().cast()) })?;

        // SAFETY: `sigemptyset` will have initialized `set`
        Ok(unsafe { set.assume_init() })
    }

    /// Add a signal to this set
    pub(crate) fn add(&mut self, sig: c_int) -> io::Result<()> {
        // SAFETY: we pass a valid mutable pointer to `sigaddset`
        cerr(unsafe { libc::sigaddset(&mut self.raw, sig) })?;

        Ok(())
    }

    /// Get a reference to the inner sigset_t.
    pub(crate) fn as_ref(&self) -> &libc::sigset_t {
        &self.raw
    }

    fn sigprocmask(&self, how: c_int) -> io::Result<Self> {
        let mut original_set = MaybeUninit::<Self>::zeroed();

        // SAFETY: we pass a valid mutable pointer to `pthread_sigmask`
        cerr(unsafe { libc::pthread_sigmask(how, &self.raw, original_set.as_mut_ptr().cast()) })?;

        // SAFETY: `sigprocmask` will have initialized `set`
        Ok(unsafe { original_set.assume_init() })
    }

    /// Block all the signals in this set and return the previous set of blocked signals.
    ///
    /// After calling this function successfully, the set of blocked signals will be the union of
    /// the previous set of blocked signals and this set.
    pub(crate) fn block(&self) -> io::Result<Self> {
        self.sigprocmask(libc::SIG_BLOCK)
    }

    /// Block only the signals that are in this set and return the previous set of blocked signals.
    ///
    /// After calling this function successfully, the set of blocked signals will be the exactly
    /// this set.
    pub(crate) fn set_mask(&self) -> io::Result<Self> {
        self.sigprocmask(libc::SIG_SETMASK)
    }
}
