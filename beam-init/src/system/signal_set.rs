// Based on sudo-rs code which is
// Copyright (c) 2022-2026 Trifecta Tech Foundation and contributors
// SPDX-License-Identifier: Apache-2.0
// this has been changed to add as_ptr, replace empty+add with new, use
// pthread_sigmask instead of sigprocmask and to remove full and unblock.

use super::cerr;

use std::ffi::c_int;
use std::io;
use std::mem::MaybeUninit;

// A signal set that can be used to mask signals.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct SignalSet {
    raw: libc::sigset_t,
}

impl SignalSet {
    /// Create a set with the given signals added
    pub fn new(signals: &[c_int]) -> io::Result<Self> {
        let mut set = MaybeUninit::<Self>::zeroed();

        // SAFETY: we pass a valid mutable pointer to `sigemptyset`
        cerr(unsafe { libc::sigemptyset(set.as_mut_ptr().cast()) })?;

        // SAFETY: `sigemptyset` will have initialized `set`
        let mut set = unsafe { set.assume_init() };

        for &signum in signals {
            // SAFETY: we pass a valid mutable pointer to `sigaddset`
            cerr(unsafe { libc::sigaddset(&mut set.raw, signum) })?;
        }

        Ok(set)
    }

    /// Get a reference to the inner sigset_t.
    #[expect(clippy::should_implement_trait)]
    pub fn as_ref(&self) -> &libc::sigset_t {
        &self.raw
    }

    fn thread_sigmask(&self, how: c_int) -> io::Result<Self> {
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
    pub fn block(&self) -> io::Result<Self> {
        self.thread_sigmask(libc::SIG_BLOCK)
    }

    /// Block only the signals that are in this set and return the previous set of blocked signals.
    ///
    /// After calling this function successfully, the set of blocked signals will be the exactly
    /// this set.
    pub fn set_mask(&self) -> io::Result<Self> {
        self.thread_sigmask(libc::SIG_SETMASK)
    }
}
