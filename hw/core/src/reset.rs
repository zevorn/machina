use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetType {
    Cold,
    Warm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetPhase {
    reset_type: ResetType,
}

impl ResetPhase {
    pub fn reset_type(self) -> ResetType {
        self.reset_type
    }
}

pub trait Resettable {
    fn reset_enter(&self, _phase: ResetPhase) {}
    fn reset_hold(&self, _phase: ResetPhase) {}
    fn reset_exit(&self, _phase: ResetPhase) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetError {
    Reentrant,
}

impl fmt::Display for ResetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reentrant => write!(f, "reset is already in progress"),
        }
    }
}

impl std::error::Error for ResetError {}

#[derive(Default)]
pub struct MResetController {
    in_reset: AtomicBool,
}

struct ResetGuard<'a> {
    in_reset: &'a AtomicBool,
}

impl Drop for ResetGuard<'_> {
    fn drop(&mut self) {
        self.in_reset.store(false, Ordering::Release);
    }
}

impl MResetController {
    pub fn reset<'a, I>(
        &self,
        devices: I,
        reset_type: ResetType,
    ) -> Result<(), ResetError>
    where
        I: IntoIterator<Item = &'a dyn Resettable>,
    {
        if self
            .in_reset
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(ResetError::Reentrant);
        }
        let _guard = ResetGuard {
            in_reset: &self.in_reset,
        };

        let devices: Vec<&dyn Resettable> = devices.into_iter().collect();
        let phase = ResetPhase { reset_type };

        for device in &devices {
            device.reset_enter(phase);
        }
        for device in &devices {
            device.reset_hold(phase);
        }
        for device in &devices {
            device.reset_exit(phase);
        }

        Ok(())
    }
}
