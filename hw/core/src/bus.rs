// Bus-attached device interface.

use std::any::Any;
use std::fmt;

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectError, MObjectState};
use machina_memory::address_space::AddressSpace;
use machina_memory::region::MemoryRegion;

use crate::irq::IrqLine;
use crate::mdev::{MDevice, MDeviceError, MDeviceState};
use crate::qdev::Device;

/// A device that can be attached to a memory-mapped bus.
pub trait BusDevice: Device {
    fn read(&self, offset: u64, size: u32) -> u64;
    fn write(&mut self, offset: u64, size: u32, val: u64);
}

// -- SysBus --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysBusMapping {
    pub owner: String,
    pub name: String,
    pub base: GPA,
    pub size: u64,
}

struct RegisteredSysBusMapping {
    owner: String,
    name: String,
    base: Option<GPA>,
    size: u64,
    region: Option<MemoryRegion>,
}

impl RegisteredSysBusMapping {
    fn desc(&self) -> Option<SysBusMapping> {
        self.base.map(|base| SysBusMapping {
            owner: self.owner.clone(),
            name: self.name.clone(),
            base,
            size: self.size,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysBusError {
    Device(MDeviceError),
    Object(MObjectError),
    MissingParentBus,
    ParentBusMismatch { attached: String, expected: String },
    DetachedObject(String),
    MissingMmio(String),
    MissingMmioMapping(String),
    InvalidMmioSlot(usize),
    MmioAlreadyMapped(String),
    MissingRealizedMapping(String),
    MissingIrq(String),
    InvalidIrqSlot(usize),
    IrqAlreadyConnected(String),
    MmioOverlap { existing: String, requested: String },
}

impl fmt::Display for SysBusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Device(err) => write!(f, "{err}"),
            Self::Object(err) => write!(f, "{err}"),
            Self::MissingParentBus => {
                write!(f, "sysbus device must attach to a parent bus first")
            }
            Self::ParentBusMismatch { attached, expected } => {
                write!(
                    f,
                    "sysbus device is attached to '{attached}', expected '{expected}'"
                )
            }
            Self::DetachedObject(device) => {
                write!(
                    f,
                    "sysbus device '{device}' must be attached to the MOM tree before realize"
                )
            }
            Self::MissingMmio(device) => {
                write!(f, "sysbus device '{device}' has no MMIO mappings")
            }
            Self::MissingMmioMapping(name) => {
                write!(f, "sysbus MMIO slot '{name}' has no board mapping")
            }
            Self::InvalidMmioSlot(slot) => {
                write!(f, "sysbus MMIO slot {slot} does not exist")
            }
            Self::MmioAlreadyMapped(name) => {
                write!(f, "sysbus MMIO slot '{name}' is already mapped")
            }
            Self::MissingRealizedMapping(name) => {
                write!(f, "sysbus realized mapping '{name}' is missing")
            }
            Self::MissingIrq(device) => {
                write!(
                    f,
                    "sysbus device '{device}' has unconnected IRQ outputs"
                )
            }
            Self::InvalidIrqSlot(slot) => {
                write!(f, "sysbus IRQ slot {slot} does not exist")
            }
            Self::IrqAlreadyConnected(device) => {
                write!(f, "sysbus IRQ slot for '{device}' is already connected")
            }
            Self::MmioOverlap {
                existing,
                requested,
            } => {
                write!(
                    f,
                    "sysbus MMIO mapping '{requested}' overlaps existing '{existing}'"
                )
            }
        }
    }
}

impl std::error::Error for SysBusError {}

impl From<MDeviceError> for SysBusError {
    fn from(value: MDeviceError) -> Self {
        Self::Device(value)
    }
}

impl From<MObjectError> for SysBusError {
    fn from(value: MObjectError) -> Self {
        Self::Object(value)
    }
}

pub struct SysBusDeviceState {
    device: MDeviceState,
    mappings: Vec<RegisteredSysBusMapping>,
    irq_slots: Vec<Option<IrqLine>>,
    irq_outputs: Vec<IrqLine>,
}

impl SysBusDeviceState {
    pub fn new(local_id: &str) -> Self {
        Self {
            device: MDeviceState::new(local_id),
            mappings: Vec::new(),
            irq_slots: Vec::new(),
            irq_outputs: Vec::new(),
        }
    }

    pub fn device(&self) -> &MDeviceState {
        &self.device
    }

    pub fn device_mut(&mut self) -> &mut MDeviceState {
        &mut self.device
    }

    pub fn attach_to_bus(
        &mut self,
        bus: &mut SysBus,
    ) -> Result<(), SysBusError> {
        self.device.set_parent_bus(&bus.name)?;
        bus.attach_child(self.device.object_mut())?;
        Ok(())
    }

    pub fn register_mmio(
        &mut self,
        region: MemoryRegion,
        base: GPA,
    ) -> Result<(), SysBusError> {
        let slot = self.declare_mmio(region)?;
        if let Err(err) = self.map_mmio(slot, base) {
            self.mappings.remove(slot);
            return Err(err);
        }
        Ok(())
    }

    pub fn declare_mmio(
        &mut self,
        region: MemoryRegion,
    ) -> Result<usize, SysBusError> {
        if self.device.is_realized() {
            return Err(MDeviceError::LateMutation("sysbus_mmio").into());
        }

        let slot = self.mappings.len();
        self.mappings.push(RegisteredSysBusMapping {
            owner: self.device.local_id().to_string(),
            name: region.name.clone(),
            size: region.size,
            base: None,
            region: Some(region),
        });
        Ok(slot)
    }

    pub fn map_mmio(
        &mut self,
        slot: usize,
        base: GPA,
    ) -> Result<(), SysBusError> {
        if self.device.is_realized() {
            return Err(MDeviceError::LateMutation("sysbus_mmio").into());
        }

        let requested = {
            let mapping = self
                .mappings
                .get(slot)
                .ok_or(SysBusError::InvalidMmioSlot(slot))?;
            if mapping.base.is_some() {
                return Err(SysBusError::MmioAlreadyMapped(
                    mapping.name.clone(),
                ));
            }
            SysBusMapping {
                owner: mapping.owner.clone(),
                name: mapping.name.clone(),
                base,
                size: mapping.size,
            }
        };

        for (index, existing) in self.mappings.iter().enumerate() {
            if index == slot {
                continue;
            }
            if let Some(existing) = existing.desc() {
                if ranges_overlap(&existing, &requested) {
                    return Err(SysBusError::MmioOverlap {
                        existing: existing.name,
                        requested: requested.name,
                    });
                }
            }
        }

        self.mappings[slot].base = Some(base);
        Ok(())
    }

    pub fn register_irq(&mut self, irq: IrqLine) -> Result<(), SysBusError> {
        let slot = self.declare_irq()?;
        self.connect_irq(slot, irq)
    }

    pub fn declare_irq(&mut self) -> Result<usize, SysBusError> {
        if self.device.is_realized() {
            return Err(MDeviceError::LateMutation("sysbus_irq").into());
        }
        let slot = self.irq_slots.len();
        self.irq_slots.push(None);
        Ok(slot)
    }

    pub fn connect_irq(
        &mut self,
        slot: usize,
        irq: IrqLine,
    ) -> Result<(), SysBusError> {
        if self.device.is_realized() {
            return Err(MDeviceError::LateMutation("sysbus_irq").into());
        }

        let irq_slot = self
            .irq_slots
            .get_mut(slot)
            .ok_or(SysBusError::InvalidIrqSlot(slot))?;
        if irq_slot.is_some() {
            return Err(SysBusError::IrqAlreadyConnected(
                self.device.local_id().to_string(),
            ));
        }

        *irq_slot = Some(irq);
        self.refresh_irq_outputs();
        Ok(())
    }

    fn refresh_irq_outputs(&mut self) {
        self.irq_outputs =
            self.irq_slots.iter().filter_map(Clone::clone).collect();
    }

    pub fn mappings(&self) -> Vec<SysBusMapping> {
        self.mappings
            .iter()
            .filter_map(RegisteredSysBusMapping::desc)
            .collect()
    }

    pub fn irq_outputs(&self) -> &[IrqLine] {
        &self.irq_outputs
    }

    pub fn realize_onto(
        &mut self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        if self.device.is_realized() {
            return Err(MDeviceError::AlreadyRealized.into());
        }

        if self.mappings.is_empty() {
            return Err(SysBusError::MissingMmio(
                self.device.local_id().to_string(),
            ));
        }

        for mapping in &self.mappings {
            if mapping.base.is_none() {
                return Err(SysBusError::MissingMmioMapping(
                    mapping.name.clone(),
                ));
            }
        }

        if self.irq_slots.iter().any(Option::is_none) {
            return Err(SysBusError::MissingIrq(
                self.device.local_id().to_string(),
            ));
        }

        match self.device.parent_bus() {
            Some(attached) if attached == bus.name => {}
            Some(attached) => {
                return Err(SysBusError::ParentBusMismatch {
                    attached: attached.to_string(),
                    expected: bus.name.clone(),
                });
            }
            None => return Err(SysBusError::MissingParentBus),
        }

        for mapping in &self.mappings {
            bus.validate_mapping(
                &mapping
                    .desc()
                    .expect("mapped sysbus slot must have a descriptor"),
            )?;
        }

        self.device.mark_realized()?;

        for mapping in &mut self.mappings {
            let desc = mapping
                .desc()
                .expect("mapped sysbus slot must have a descriptor");
            bus.record_mapping(desc.clone());
            let region = mapping
                .region
                .take()
                .expect("sysbus MMIO region must exist before realize");
            address_space.root_mut().add_subregion(region, desc.base);
        }

        address_space.update_flat_view();
        Ok(())
    }

    pub fn unrealize_from(
        &mut self,
        bus: &mut SysBus,
        address_space: &mut AddressSpace,
    ) -> Result<(), SysBusError> {
        if !self.device.is_realized() {
            return Err(MDeviceError::NotRealized.into());
        }

        for mapping in self.mappings.iter_mut().rev() {
            let desc = mapping
                .desc()
                .expect("realized sysbus slot must have a descriptor");
            let region = address_space
                .remove_subregion(desc.base, &desc.name)
                .ok_or_else(|| {
                    SysBusError::MissingRealizedMapping(desc.name.clone())
                })?;
            mapping.region = Some(region);
            bus.remove_mapping(&desc).ok_or_else(|| {
                SysBusError::MissingRealizedMapping(desc.name.clone())
            })?;
        }

        self.device.mark_unrealized()?;
        Ok(())
    }
}

#[macro_export]
macro_rules! machina_impl_sysbus_device {
    ($ty:ty, $field:ident) => {
        impl $crate::machina_core::mobject::MObject for $ty {
            fn mobject_state(
                &self,
            ) -> &$crate::machina_core::mobject::MObjectState {
                $crate::machina_core::mobject::MObject::mobject_state(
                    &self.$field,
                )
            }

            fn mobject_state_mut(
                &mut self,
            ) -> &mut $crate::machina_core::mobject::MObjectState {
                $crate::machina_core::mobject::MObject::mobject_state_mut(
                    &mut self.$field,
                )
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        impl $crate::mdev::MDevice for $ty {
            fn mdevice_state(&self) -> &$crate::mdev::MDeviceState {
                $crate::mdev::MDevice::mdevice_state(&self.$field)
            }

            fn mdevice_state_mut(&mut self) -> &mut $crate::mdev::MDeviceState {
                $crate::mdev::MDevice::mdevice_state_mut(&mut self.$field)
            }
        }
    };
}

#[macro_export]
macro_rules! machina_std_mutex_sysbus_accessors {
    ($field:ident) => {
        pub fn attach_to_bus(
            &self,
            bus: &mut $crate::bus::SysBus,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().attach_to_bus(bus)
        }

        pub fn register_mmio(
            &self,
            region: $crate::machina_memory::region::MemoryRegion,
            base: $crate::machina_core::address::GPA,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().register_mmio(region, base)
        }

        pub fn declare_mmio(
            &self,
            region: $crate::machina_memory::region::MemoryRegion,
        ) -> Result<usize, $crate::bus::SysBusError> {
            self.$field.lock().unwrap().declare_mmio(region)
        }

        pub fn map_mmio(
            &self,
            slot: usize,
            base: $crate::machina_core::address::GPA,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().map_mmio(slot, base)
        }

        pub fn register_irq(
            &self,
            irq: $crate::irq::IrqLine,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().register_irq(irq)
        }

        pub fn declare_irq(&self) -> Result<usize, $crate::bus::SysBusError> {
            self.$field.lock().unwrap().declare_irq()
        }

        pub fn connect_irq(
            &self,
            slot: usize,
            irq: $crate::irq::IrqLine,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().connect_irq(slot, irq)
        }

        pub fn realize_onto(
            &self,
            bus: &mut $crate::bus::SysBus,
            address_space: &mut $crate::machina_memory::address_space::AddressSpace,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().realize_onto(bus, address_space)
        }

        pub fn unrealize_from(
            &self,
            bus: &mut $crate::bus::SysBus,
            address_space: &mut $crate::machina_memory::address_space::AddressSpace,
        ) -> Result<(), $crate::bus::SysBusError> {
            self.$field.lock().unwrap().unrealize_from(bus, address_space)
        }

        pub fn realized(&self) -> bool {
            let guard = self.$field.lock().unwrap();
            $crate::mdev::MDevice::is_realized(&*guard)
        }

        pub fn with_mdevice<T>(
            &self,
            f: impl FnOnce(&dyn $crate::mdev::MDevice) -> T,
        ) -> T {
            let guard = self.$field.lock().unwrap();
            f(&*guard)
        }

        pub fn object_info(
            &self,
        ) -> $crate::machina_core::mobject::MObjectInfo {
            let guard = self.$field.lock().unwrap();
            $crate::machina_core::mobject::MObject::object_info(&*guard)
        }
    };
}

impl MObject for SysBusDeviceState {
    fn mobject_state(&self) -> &MObjectState {
        self.device.mobject_state()
    }

    fn mobject_state_mut(&mut self) -> &mut MObjectState {
        self.device.mobject_state_mut()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl MDevice for SysBusDeviceState {
    fn mdevice_state(&self) -> &MDeviceState {
        &self.device
    }

    fn mdevice_state_mut(&mut self) -> &mut MDeviceState {
        &mut self.device
    }
}

/// System bus — the default bus for platform devices.
pub struct SysBus {
    object: MObjectState,
    pub name: String,
    mappings: Vec<SysBusMapping>,
}

impl SysBus {
    pub fn new(name: &str) -> Self {
        Self {
            object: MObjectState::new_root(name)
                .expect("sysbus local_id must be valid"),
            name: name.to_string(),
            mappings: Vec::new(),
        }
    }

    pub fn attach_to_parent(
        &mut self,
        parent: &mut MObjectState,
    ) -> Result<(), SysBusError> {
        parent.attach_child(&mut self.object)?;
        Ok(())
    }

    pub fn attach_child(
        &mut self,
        child: &mut MObjectState,
    ) -> Result<(), SysBusError> {
        self.object.attach_child(child)?;
        Ok(())
    }

    fn validate_mapping(
        &self,
        requested: &SysBusMapping,
    ) -> Result<(), SysBusError> {
        for existing in &self.mappings {
            if ranges_overlap(existing, requested) {
                return Err(SysBusError::MmioOverlap {
                    existing: existing.name.clone(),
                    requested: requested.name.clone(),
                });
            }
        }
        Ok(())
    }

    fn record_mapping(&mut self, mapping: SysBusMapping) {
        self.mappings.push(mapping);
    }

    fn remove_mapping(
        &mut self,
        target: &SysBusMapping,
    ) -> Option<SysBusMapping> {
        let index =
            self.mappings.iter().position(|mapping| mapping == target)?;
        Some(self.mappings.remove(index))
    }

    pub fn mappings(&self) -> &[SysBusMapping] {
        &self.mappings
    }
}

impl MObject for SysBus {
    fn mobject_state(&self) -> &MObjectState {
        &self.object
    }

    fn mobject_state_mut(&mut self) -> &mut MObjectState {
        &mut self.object
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn ranges_overlap(lhs: &SysBusMapping, rhs: &SysBusMapping) -> bool {
    let lhs_end = lhs.base.0 + lhs.size;
    let rhs_end = rhs.base.0 + rhs.size;
    lhs.base.0 < rhs_end && rhs.base.0 < lhs_end
}
