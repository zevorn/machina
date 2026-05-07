use std::any::Any;
use std::sync::{Arc, Mutex};

use machina_core::address::GPA;
use machina_core::mobject::{MObject, MObjectState};
use machina_hw_core::bus::SysBusDeviceState;
use machina_hw_core::bus::{SysBus, SysBusError};
use machina_hw_core::mdev::{MDevice, MDeviceError, MDeviceState};
use machina_hw_core::property::{MPropertySpec, MPropertyType, MPropertyValue};
use machina_hw_core::reset::{
    MResetController, ResetPhase, ResetType, Resettable,
};
use machina_hw_core::typeinfo::{
    MObjectKind, MTypeError, MTypeInfo, MTypeRegistry,
};
use machina_memory::region::{MemoryRegion, MmioOps};

struct NoopMmio;

impl MmioOps for NoopMmio {
    fn read(&self, _offset: u64, _size: u32) -> u64 {
        0
    }

    fn write(&self, _offset: u64, _size: u32, _val: u64) {}
}

struct RegistryFixture {
    state: MDeviceState,
}

impl RegistryFixture {
    fn boxed(local_id: &str) -> Box<dyn MDevice> {
        Box::new(Self {
            state: MDeviceState::new(local_id),
        })
    }
}

impl MObject for RegistryFixture {
    fn mobject_state(&self) -> &MObjectState {
        self.state.object()
    }

    fn mobject_state_mut(&mut self) -> &mut MObjectState {
        self.state.object_mut()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl MDevice for RegistryFixture {
    fn mdevice_state(&self) -> &MDeviceState {
        &self.state
    }

    fn mdevice_state_mut(&mut self) -> &mut MDeviceState {
        &mut self.state
    }
}

#[test]
fn test_type_registry_registers_metadata_properties_and_factory() {
    let mut registry = MTypeRegistry::default();
    registry
        .register(MTypeInfo::object("object"))
        .expect("register root object type");
    registry
        .register_strict(
            MTypeInfo::device("test-device")
                .with_parent("object")
                .with_property(
                    MPropertySpec::new("enabled", MPropertyType::Bool)
                        .default(MPropertyValue::Bool(true)),
                )
                .with_device_factory(RegistryFixture::boxed),
        )
        .expect("register device type");

    let info = registry.get("test-device").expect("type metadata exists");
    assert_eq!(info.kind(), MObjectKind::Device);
    assert_eq!(info.parent_type(), Some("object"));
    assert_eq!(info.properties().len(), 1);
    assert_eq!(info.properties()[0].name, "enabled");

    let dev = registry
        .create_device("test-device", "dev0")
        .expect("factory creates device");
    assert_eq!(dev.local_id(), "dev0");
    assert_eq!(dev.object_path(), None);
}

#[test]
fn test_type_registry_rejects_duplicate_and_missing_parent() {
    let mut registry = MTypeRegistry::default();
    registry
        .register(MTypeInfo::object("object"))
        .expect("register root object type");

    let duplicate = registry
        .register(MTypeInfo::object("object"))
        .expect_err("duplicate type must fail");
    assert_eq!(duplicate, MTypeError::DuplicateType("object".to_string()));

    let missing_parent = registry
        .register_strict(MTypeInfo::device("child").with_parent("missing"))
        .expect_err("missing parent must fail");
    assert_eq!(
        missing_parent,
        MTypeError::MissingParentType("missing".to_string())
    );
}

#[test]
fn test_type_registry_rejects_non_device_factory_and_duplicate_property_schema()
{
    let mut registry = MTypeRegistry::default();
    registry
        .register(MTypeInfo::object("object"))
        .expect("register root object type");

    let factory_on_object = registry
        .register(
            MTypeInfo::object("bad-object")
                .with_device_factory(RegistryFixture::boxed),
        )
        .expect_err("object type must not expose a device factory");
    assert_eq!(
        factory_on_object,
        MTypeError::FactoryOnNonDevice("bad-object".to_string())
    );

    let duplicate_property = registry
        .register_strict(
            MTypeInfo::device("bad-device")
                .with_parent("object")
                .with_property(MPropertySpec::new("dup", MPropertyType::Bool))
                .with_property(MPropertySpec::new("dup", MPropertyType::U32)),
        )
        .expect_err("duplicate property schema must fail");
    assert_eq!(
        duplicate_property,
        MTypeError::DuplicateProperty {
            type_name: "bad-device".to_string(),
            property: "dup".to_string(),
        }
    );
}

#[test]
fn test_property_schema_macro_generates_default_required_and_dynamic_specs() {
    let specs = machina_hw_core::machina_property_specs![
        bool enabled = true,
        u32 threshold required,
        string label dynamic,
    ];
    let info = MTypeInfo::device("macro-device")
        .with_parent("object")
        .with_properties(specs.clone());

    assert_eq!(info.properties().len(), 3);

    let mut state = MDeviceState::new("macro0");
    for spec in specs {
        state.define_property(spec).expect("define macro property");
    }

    assert_eq!(state.property("enabled"), Some(&MPropertyValue::Bool(true)));
    assert_eq!(
        state
            .mark_realized()
            .expect_err("missing required property"),
        MDeviceError::MissingRequiredProperty("threshold".to_string())
    );

    state
        .set_property("threshold", MPropertyValue::U32(42))
        .expect("set required property");
    state
        .mark_realized()
        .expect("realize after required property");

    assert_eq!(
        state
            .set_property("enabled", MPropertyValue::Bool(false))
            .expect_err("static property late mutation"),
        MDeviceError::LateMutation("property")
    );
    state
        .set_property("label", MPropertyValue::String("hot".to_string()))
        .expect("dynamic property late mutation");
    assert_eq!(
        state.property("label"),
        Some(&MPropertyValue::String("hot".to_string()))
    );
}

struct MacroObject {
    state: MObjectState,
}

machina_core::machina_impl_mobject!(MacroObject, state);

struct MacroDevice {
    state: MDeviceState,
}

machina_hw_core::machina_impl_mdevice!(MacroDevice, state);

struct MacroSysBusDevice {
    state: SysBusDeviceState,
}

machina_hw_core::machina_impl_sysbus_device!(MacroSysBusDevice, state);

#[test]
fn test_object_device_and_sysbus_macros_remove_trait_boilerplate() {
    let obj = MacroObject {
        state: MObjectState::new_root("machine").expect("root object"),
    };
    assert_eq!(obj.object_path(), Some("/machine"));

    let dev = MacroDevice {
        state: MDeviceState::new("tmp105"),
    };
    assert_eq!(dev.local_id(), "tmp105");
    assert!(!dev.is_realized());

    let sysbus = MacroSysBusDevice {
        state: SysBusDeviceState::new("uart0"),
    };
    assert_eq!(sysbus.local_id(), "uart0");
    assert!(!sysbus.is_realized());
}

struct LockedSysBusFixture {
    state: Mutex<SysBusDeviceState>,
}

impl LockedSysBusFixture {
    fn new(local_id: &str) -> Self {
        Self {
            state: Mutex::new(SysBusDeviceState::new(local_id)),
        }
    }

    machina_hw_core::machina_std_mutex_sysbus_accessors!(state);
}

struct LockedMDeviceFixture {
    mdevice: parking_lot::Mutex<MDeviceState>,
}

impl LockedMDeviceFixture {
    fn new(local_id: &str) -> Self {
        Self {
            mdevice: parking_lot::Mutex::new(MDeviceState::new(local_id)),
        }
    }

    machina_hw_core::machina_parking_lot_mdevice_accessors!(mdevice);
}

#[test]
fn test_locked_state_accessor_macros_cover_common_device_wrappers() {
    let sysbus = LockedSysBusFixture::new("uart0");
    let slot = sysbus
        .declare_mmio(MemoryRegion::io("uart0-mmio", 0x100, Arc::new(NoopMmio)))
        .expect("declare MMIO");
    sysbus
        .map_mmio(slot, GPA::new(0x1000_0000))
        .expect("map MMIO");
    sysbus.with_mdevice(|dev| assert_eq!(dev.local_id(), "uart0"));
    assert_eq!(sysbus.object_info().local_id, "uart0");

    let dev = LockedMDeviceFixture::new("tmp105");
    assert!(!dev.realized());
    dev.with_mdevice(|state| assert_eq!(state.local_id(), "tmp105"));
    dev.realize().expect("realize mdevice");
    assert!(dev.realized());
    dev.unrealize().expect("unrealize mdevice");
    assert!(!dev.realized());
    assert_eq!(dev.object_info().local_id, "tmp105");
}

struct RecordingReset {
    name: &'static str,
    events: Arc<Mutex<Vec<String>>>,
}

impl Resettable for RecordingReset {
    fn reset_enter(&self, phase: ResetPhase) {
        self.events.lock().unwrap().push(format!(
            "{}.enter.{:?}",
            self.name,
            phase.reset_type()
        ));
    }

    fn reset_hold(&self, phase: ResetPhase) {
        self.events.lock().unwrap().push(format!(
            "{}.hold.{:?}",
            self.name,
            phase.reset_type()
        ));
    }

    fn reset_exit(&self, phase: ResetPhase) {
        self.events.lock().unwrap().push(format!(
            "{}.exit.{:?}",
            self.name,
            phase.reset_type()
        ));
    }
}

#[test]
fn test_reset_controller_runs_phase_order_across_devices() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let first = RecordingReset {
        name: "first",
        events: Arc::clone(&events),
    };
    let second = RecordingReset {
        name: "second",
        events: Arc::clone(&events),
    };
    let controller = MResetController::default();

    controller
        .reset([&first as &dyn Resettable, &second], ResetType::Cold)
        .expect("reset graph");

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "first.enter.Cold".to_string(),
            "second.enter.Cold".to_string(),
            "first.hold.Cold".to_string(),
            "second.hold.Cold".to_string(),
            "first.exit.Cold".to_string(),
            "second.exit.Cold".to_string(),
        ]
    );
}

struct ReentrantReset<'a> {
    controller: &'a MResetController,
    observed: Arc<Mutex<Option<String>>>,
}

impl Resettable for ReentrantReset<'_> {
    fn reset_enter(&self, _phase: ResetPhase) {
        let err = self
            .controller
            .reset(std::iter::empty::<&dyn Resettable>(), ResetType::Warm)
            .expect_err("nested reset must be rejected");
        *self.observed.lock().unwrap() = Some(err.to_string());
    }
}

#[test]
fn test_reset_controller_rejects_reentrant_reset() {
    let controller = MResetController::default();
    let observed = Arc::new(Mutex::new(None));
    let device = ReentrantReset {
        controller: &controller,
        observed: Arc::clone(&observed),
    };

    controller
        .reset([&device as &dyn Resettable], ResetType::Cold)
        .expect("outer reset completes");

    assert_eq!(
        *observed.lock().unwrap(),
        Some("reset is already in progress".to_string())
    );
}

struct TopologyMutationReset {
    state: Arc<Mutex<SysBusDeviceState>>,
    observed: Arc<Mutex<Option<SysBusError>>>,
}

impl Resettable for TopologyMutationReset {
    fn reset_hold(&self, _phase: ResetPhase) {
        let err = self
            .state
            .lock()
            .unwrap()
            .declare_mmio(MemoryRegion::io(
                "late-mmio",
                0x100,
                Arc::new(NoopMmio),
            ))
            .expect_err("reset must not change realized sysbus topology");
        *self.observed.lock().unwrap() = Some(err);
    }
}

#[test]
fn test_reset_phase_cannot_change_realized_sysbus_topology() {
    let state = Arc::new(Mutex::new(SysBusDeviceState::new("uart0")));
    {
        let mut guard = state.lock().unwrap();
        let slot = guard
            .declare_mmio(MemoryRegion::io(
                "uart0-mmio",
                0x100,
                Arc::new(NoopMmio),
            ))
            .expect("declare MMIO");
        guard
            .map_mmio(slot, GPA::new(0x1000_0000))
            .expect("map MMIO");
    }

    let mut bus = SysBus::new("sysbus0");
    let mut address_space = machina_memory::address_space::AddressSpace::new(
        MemoryRegion::container("system", u64::MAX),
    );
    state
        .lock()
        .unwrap()
        .attach_to_bus(&mut bus)
        .expect("attach sysbus device");
    state
        .lock()
        .unwrap()
        .realize_onto(&mut bus, &mut address_space)
        .expect("realize sysbus device");

    let observed = Arc::new(Mutex::new(None));
    let device = TopologyMutationReset {
        state: Arc::clone(&state),
        observed: Arc::clone(&observed),
    };
    MResetController::default()
        .reset([&device as &dyn Resettable], ResetType::Cold)
        .expect("reset completes");

    assert_eq!(
        *observed.lock().unwrap(),
        Some(SysBusError::Device(MDeviceError::LateMutation(
            "sysbus_mmio"
        )))
    );
    assert_eq!(bus.mappings().len(), 1);
    assert!(address_space.is_mapped(GPA::new(0x1000_0000), 4));
}
