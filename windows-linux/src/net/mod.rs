//! The operating system side: list ports, set a static address, go back to DHCP.

use crate::config::ManualConfig;
use std::net::Ipv4Addr;
use std::process::Command;

// Both are plain Rust with no OS bindings, so they compile (and their parsers
// are tested) everywhere; only the matching one is used.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

// Lets the UI be developed on a Mac. Releases use the Swift app there.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dhcp,
    Manual,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    /// What the OS tool needs to address it (Windows interface index, Linux
    /// device, macOS service name).
    pub id: String,
    /// What the user sees and what saved addresses remember.
    pub name: String,
    /// Hardware description or device name.
    pub detail: String,
    pub wired: bool,
    /// Cable plugged in and link up.
    pub connected: bool,
    pub mode: Mode,
    pub address: Option<Ipv4Addr>,
    pub prefix: Option<u8>,
    pub gateway: Option<Ipv4Addr>,
}

pub fn adapters() -> Result<Vec<Adapter>, String> {
    platform::adapters()
}

pub fn set_static(adapter: &Adapter, config: &ManualConfig) -> Result<(), String> {
    platform::set_static(adapter, config)
}

pub fn set_dhcp(adapter: &Adapter) -> Result<(), String> {
    platform::set_dhcp(adapter)
}

/// Runs a tool and returns stdout, or its output as the error message.
pub(crate) fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).env("LC_ALL", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("Could not run {program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = format!("{}\n{}", stderr.trim(), stdout.trim()).trim().to_string();
        Err(if msg.is_empty() { format!("{program} failed ({})", out.status) } else { msg })
    }
}
