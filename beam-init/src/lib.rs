#![deny(clippy::unwrap_used)]

pub const BOOTSTRAP_NAME: &str = "bootstrap";

pub mod system;

#[derive(Debug)]
pub enum MonitorEvent {
    Stopped,
}

impl MonitorEvent {
    pub fn ser(self) -> u8 {
        match self {
            MonitorEvent::Stopped => 1,
        }
    }

    pub fn de(b: u8) -> MonitorEvent {
        match b.cast_signed() {
            1 => MonitorEvent::Stopped,
            _ => unreachable!(),
        }
    }
}
