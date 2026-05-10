/// Machine model trait and configuration.
use std::any::Any;
use std::path::PathBuf;

use crate::mobject::{MObject, MObjectState};

// TODO: use GPA in Machine trait methods (e.g. ram_base)
// use crate::address::GPA;

#[derive(Clone, Debug)]
pub struct NetdevOpts {
    pub id: String,
    pub ifname: String,
    pub mac: Option<String>,
}

impl NetdevOpts {
    /// Parse `-netdev` and optional `-device
    /// virtio-net-device` into NetdevOpts.
    ///
    /// Empty value strings (`id=`, `ifname=`, `netdev=`) are
    /// rejected up front so callers don't fail later inside
    /// device init or TAP setup. Only `tap` netdevs and
    /// `virtio-net-device` device strings are accepted; anything
    /// else returns an explicit `Err` naming the field at fault.
    pub fn parse(
        netdev_raw: &str,
        device_raw: Option<&str>,
    ) -> Result<Self, String> {
        if !netdev_raw.starts_with("tap,") {
            return Err(format!(
                "-netdev: unsupported type \
                 (expected tap): {}",
                netdev_raw
            ));
        }
        let mut id = None;
        let mut ifname = None;
        for part in netdev_raw.split(',').skip(1) {
            if let Some(v) = part.strip_prefix("id=") {
                if v.is_empty() {
                    return Err("-netdev: empty id= value".to_string());
                }
                id = Some(v.to_string());
            } else if let Some(v) = part.strip_prefix("ifname=") {
                if v.is_empty() {
                    return Err("-netdev: empty ifname= value".to_string());
                }
                ifname = Some(v.to_string());
            }
        }
        let id = id.ok_or("-netdev: missing id= parameter".to_string())?;
        let ifname =
            ifname.ok_or("-netdev: missing ifname= parameter".to_string())?;

        let mut mac = None;
        if let Some(dev) = device_raw {
            // First sub-token is the device kind. Only
            // virtio-net-device is supported here; anything else
            // is rejected explicitly so the user gets a clear
            // error rather than silently dropping the field.
            let kind = dev.split(',').next().unwrap_or("");
            if kind != "virtio-net-device" {
                return Err(format!(
                    "-device: unsupported type \
                     (expected virtio-net-device): {kind}",
                ));
            }
            let mut dev_netdev = None;
            for part in dev.split(',').skip(1) {
                if let Some(v) = part.strip_prefix("netdev=") {
                    if v.is_empty() {
                        return Err("-device virtio-net-device: \
                             empty netdev= value"
                            .to_string());
                    }
                    dev_netdev = Some(v.to_string());
                } else if let Some(v) = part.strip_prefix("mac=") {
                    if v.is_empty() {
                        return Err("-device virtio-net-device: \
                             empty mac= value"
                            .to_string());
                    }
                    mac = Some(v.to_string());
                }
            }
            let dev_netdev = dev_netdev.ok_or(
                "-device virtio-net-device: \
                 missing netdev= parameter"
                    .to_string(),
            )?;
            if dev_netdev != id {
                return Err(format!(
                    "-device: netdev={} does not \
                     match -netdev id={}",
                    dev_netdev, id
                ));
            }
        }
        Ok(Self { id, ifname, mac })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoaderSpec {
    pub file: PathBuf,
    pub addr: u64,
    pub force_raw: bool,
}

impl LoaderSpec {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut parts = raw.split(',');
        match parts.next() {
            Some("loader") => {}
            _ => {
                return Err(
                    "-device: only loader devices are supported by this parser"
                        .to_string(),
                );
            }
        }

        let mut file = None;
        let mut addr = None;
        let mut force_raw = false;
        for part in parts {
            if let Some(value) = part.strip_prefix("file=") {
                file = Some(PathBuf::from(value));
            } else if let Some(value) = part.strip_prefix("addr=") {
                let parsed = if let Some(hex) = value.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16)
                } else {
                    value.parse::<u64>()
                }
                .map_err(|_| format!("loader addr is invalid: {value}"))?;
                addr = Some(parsed);
            } else if let Some(value) = part.strip_prefix("force-raw=") {
                force_raw = matches!(value, "on" | "true" | "1");
            } else {
                return Err(format!("unsupported loader option: {part}"));
            }
        }

        Ok(Self {
            file: file.ok_or("loader: missing file=".to_string())?,
            addr: addr.ok_or("loader: missing addr=".to_string())?,
            force_raw,
        })
    }
}

pub struct MachineOpts {
    pub ram_size: u64,
    pub cpu_count: u32,
    pub kernel: Option<PathBuf>,
    pub bios: Option<PathBuf>,
    /// Boot directly in post-firmware mode (e.g. S-mode on
    /// RISC-V) with firmware services provided by the host.
    pub bios_builtin: bool,
    pub append: Option<String>,
    pub nographic: bool,
    pub drive: Option<PathBuf>,
    pub initrd: Option<PathBuf>,
    pub dtb: Option<PathBuf>,
    pub loaders: Vec<LoaderSpec>,
    pub netdev: Option<NetdevOpts>,
}

pub struct MachineState {
    object: MObjectState,
}

impl MachineState {
    pub fn new_root(local_id: &str) -> Self {
        Self {
            object: MObjectState::new_root(local_id)
                .expect("machine local_id must be valid"),
        }
    }

    pub fn object(&self) -> &MObjectState {
        &self.object
    }

    pub fn object_mut(&mut self) -> &mut MObjectState {
        &mut self.object
    }
}

impl MObject for MachineState {
    fn mobject_state(&self) -> &MObjectState {
        self.object()
    }

    fn mobject_state_mut(&mut self) -> &mut MObjectState {
        self.object_mut()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub trait Machine: Send + Sync {
    fn name(&self) -> &str;
    fn machine_state(&self) -> &MachineState;
    fn machine_state_mut(&mut self) -> &mut MachineState;
    fn init(
        &mut self,
        opts: &MachineOpts,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn reset(&mut self);
    fn pause(&mut self);
    fn resume(&mut self);
    fn shutdown(&mut self);
    fn boot(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn cpu_count(&self) -> usize;
    fn ram_size(&self) -> u64;
}
