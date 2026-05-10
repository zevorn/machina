use std::any::Any;
use std::fmt;

use machina_core::mobject::{MObject, MObjectState};

use crate::property::{MPropertySet, MPropertySpec, MPropertyValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MDeviceLifecycle {
    Created,
    Realized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MDeviceError {
    AlreadyRealized,
    NotRealized,
    LateMutation(&'static str),
    DuplicateProperty(String),
    UnknownProperty(String),
    MissingRequiredProperty(String),
    PropertyTypeMismatch {
        name: String,
        expected: crate::property::MPropertyType,
        actual: crate::property::MPropertyType,
    },
}

impl fmt::Display for MDeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRealized => write!(f, "device is already realized"),
            Self::NotRealized => write!(f, "device is not realized"),
            Self::LateMutation(what) => {
                write!(f, "cannot mutate {what} after realize")
            }
            Self::DuplicateProperty(name) => {
                write!(f, "property '{name}' is already defined")
            }
            Self::UnknownProperty(name) => {
                write!(f, "unknown property '{name}'")
            }
            Self::MissingRequiredProperty(name) => {
                write!(f, "required property '{name}' is missing")
            }
            Self::PropertyTypeMismatch {
                name,
                expected,
                actual,
            } => {
                write!(f, "property '{name}' expects {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for MDeviceError {}

pub struct MDeviceState {
    object: MObjectState,
    lifecycle: MDeviceLifecycle,
    parent_bus: Option<String>,
    properties: MPropertySet,
}

impl MDeviceState {
    /// Build a new device state in the `Created` lifecycle stage
    /// with `local_id` as the underlying object id, no parent
    /// bus, and an empty property set. Panics if `local_id` is
    /// not a valid object id.
    pub fn new(local_id: &str) -> Self {
        Self {
            object: MObjectState::new_detached(local_id)
                .expect("device local_id must be valid"),
            lifecycle: MDeviceLifecycle::Created,
            parent_bus: None,
            properties: MPropertySet::default(),
        }
    }

    /// Borrow the underlying `MObjectState` (read-only).
    pub fn object(&self) -> &MObjectState {
        &self.object
    }

    /// Mutably borrow the underlying `MObjectState`. Note that
    /// the `MDeviceState` itself does not gate object-level
    /// mutations on lifecycle.
    pub fn object_mut(&mut self) -> &mut MObjectState {
        &mut self.object
    }

    /// The device's local id as registered on its parent bus.
    pub fn local_id(&self) -> &str {
        self.object.local_id()
    }

    /// Current lifecycle stage (`Created` or `Realized`).
    pub fn lifecycle(&self) -> MDeviceLifecycle {
        self.lifecycle
    }

    /// Convenience: `true` once `mark_realized` has succeeded
    /// and `mark_unrealized` has not yet been called.
    pub fn is_realized(&self) -> bool {
        self.lifecycle == MDeviceLifecycle::Realized
    }

    /// Record which parent bus this device is attached to.
    /// Returns `LateMutation("parent_bus")` if the device is
    /// already realized.
    pub fn set_parent_bus(&mut self, bus: &str) -> Result<(), MDeviceError> {
        if self.is_realized() {
            return Err(MDeviceError::LateMutation("parent_bus"));
        }
        self.parent_bus = Some(bus.to_string());
        Ok(())
    }

    /// The parent bus name, if `set_parent_bus` has been called.
    pub fn parent_bus(&self) -> Option<&str> {
        self.parent_bus.as_deref()
    }

    /// Register a new property schema entry. Returns
    /// `LateMutation("property_schema")` if the device is
    /// already realized, or `DuplicateProperty(name)` if a
    /// property with the same name was already defined.
    pub fn define_property(
        &mut self,
        spec: MPropertySpec,
    ) -> Result<(), MDeviceError> {
        if self.is_realized() {
            return Err(MDeviceError::LateMutation("property_schema"));
        }
        self.properties.define(spec)
    }

    /// Assign `value` to the property `name`. Whether the
    /// assignment is allowed at the current lifecycle stage is
    /// decided by the property set.
    pub fn set_property(
        &mut self,
        name: &str,
        value: MPropertyValue,
    ) -> Result<(), MDeviceError> {
        self.properties.set(self.lifecycle, name, value)
    }

    /// Read the current value of property `name`, or `None` if
    /// the property is not set (and not defined with a default).
    pub fn property(&self, name: &str) -> Option<&MPropertyValue> {
        self.properties.get(name)
    }

    /// Look up the schema entry registered for property `name`.
    pub fn property_spec(&self, name: &str) -> Option<&MPropertySpec> {
        self.properties.spec(name)
    }

    /// All property names currently registered, in insertion
    /// order.
    pub fn property_names(&self) -> Vec<&str> {
        self.properties.names()
    }

    /// Verify that every required property has a value. Returns
    /// `MissingRequiredProperty(name)` for the first missing
    /// required property encountered.
    pub fn validate_properties(&self) -> Result<(), MDeviceError> {
        self.properties.validate_required()
    }

    /// Move the device from `Created` to `Realized`. Validates
    /// required properties first; returns `AlreadyRealized` if
    /// the device is already realized.
    pub fn mark_realized(&mut self) -> Result<(), MDeviceError> {
        if self.is_realized() {
            return Err(MDeviceError::AlreadyRealized);
        }
        self.validate_properties()?;
        self.lifecycle = MDeviceLifecycle::Realized;
        Ok(())
    }

    /// Move the device back from `Realized` to `Created`.
    /// Returns `NotRealized` if the device was not realized.
    pub fn mark_unrealized(&mut self) -> Result<(), MDeviceError> {
        if !self.is_realized() {
            return Err(MDeviceError::NotRealized);
        }
        self.lifecycle = MDeviceLifecycle::Created;
        Ok(())
    }
}

pub trait MDevice: MObject {
    fn mdevice_state(&self) -> &MDeviceState;
    fn mdevice_state_mut(&mut self) -> &mut MDeviceState;

    fn lifecycle(&self) -> MDeviceLifecycle {
        self.mdevice_state().lifecycle()
    }

    fn is_realized(&self) -> bool {
        self.mdevice_state().is_realized()
    }

    fn parent_bus(&self) -> Option<&str> {
        self.mdevice_state().parent_bus()
    }

    fn property_names(&self) -> Vec<&str> {
        self.mdevice_state().property_names()
    }

    fn property_spec(&self, name: &str) -> Option<&MPropertySpec> {
        self.mdevice_state().property_spec(name)
    }

    fn property(&self, name: &str) -> Option<&MPropertyValue> {
        self.mdevice_state().property(name)
    }
}

impl MObject for MDeviceState {
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

#[macro_export]
macro_rules! machina_impl_mdevice {
    ($ty:ty, $field:ident) => {
        impl $crate::machina_core::mobject::MObject for $ty {
            fn mobject_state(
                &self,
            ) -> &$crate::machina_core::mobject::MObjectState {
                self.$field.object()
            }

            fn mobject_state_mut(
                &mut self,
            ) -> &mut $crate::machina_core::mobject::MObjectState {
                self.$field.object_mut()
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
                &self.$field
            }

            fn mdevice_state_mut(&mut self) -> &mut $crate::mdev::MDeviceState {
                &mut self.$field
            }
        }
    };
}

#[macro_export]
macro_rules! machina_parking_lot_mdevice_accessors {
    ($field:ident) => {
        pub fn realize(&self) -> Result<(), $crate::mdev::MDeviceError> {
            self.$field.lock().mark_realized()
        }

        pub fn unrealize(&self) -> Result<(), $crate::mdev::MDeviceError> {
            self.$field.lock().mark_unrealized()
        }

        pub fn realized(&self) -> bool {
            self.$field.lock().is_realized()
        }

        pub fn with_mdevice<T>(
            &self,
            f: impl FnOnce(&$crate::mdev::MDeviceState) -> T,
        ) -> T {
            let guard = self.$field.lock();
            f(&guard)
        }

        pub fn object_info(
            &self,
        ) -> $crate::machina_core::mobject::MObjectInfo {
            let guard = self.$field.lock();
            $crate::machina_core::mobject::MObject::object_info(&*guard)
        }
    };
}

#[macro_export]
macro_rules! machina_std_mutex_mdevice_accessors {
    ($field:ident) => {
        pub fn realize(&self) -> Result<(), $crate::mdev::MDeviceError> {
            self.$field.lock().unwrap().mark_realized()
        }

        pub fn unrealize(&self) -> Result<(), $crate::mdev::MDeviceError> {
            self.$field.lock().unwrap().mark_unrealized()
        }

        pub fn realized(&self) -> bool {
            self.$field.lock().unwrap().is_realized()
        }

        pub fn with_mdevice<T>(
            &self,
            f: impl FnOnce(&$crate::mdev::MDeviceState) -> T,
        ) -> T {
            let guard = self.$field.lock().unwrap();
            f(&guard)
        }

        pub fn object_info(
            &self,
        ) -> $crate::machina_core::mobject::MObjectInfo {
            let guard = self.$field.lock().unwrap();
            $crate::machina_core::mobject::MObject::object_info(&*guard)
        }
    };
}

/// Builds typed property schema declarations.
///
/// ```compile_fail
/// let _ = machina_hw_core::machina_property_specs![
///     bool enabled = "yes",
/// ];
/// ```
#[macro_export]
macro_rules! machina_property_specs {
    () => {
        Vec::new()
    };
    ($kind:ident $name:ident = $value:expr, $($tail:tt)*) => {{
        let mut specs = vec![
            $crate::machina_property_spec!($kind $name = $value)
        ];
        specs.extend($crate::machina_property_specs![$($tail)*]);
        specs
    }};
    ($kind:ident $name:ident = $value:expr $(,)?) => {{
        vec![$crate::machina_property_spec!($kind $name = $value)]
    }};
    ($kind:ident $name:ident required, $($tail:tt)*) => {{
        let mut specs = vec![
            $crate::machina_property_spec!($kind $name required)
        ];
        specs.extend($crate::machina_property_specs![$($tail)*]);
        specs
    }};
    ($kind:ident $name:ident required $(,)?) => {{
        vec![$crate::machina_property_spec!($kind $name required)]
    }};
    ($kind:ident $name:ident dynamic, $($tail:tt)*) => {{
        let mut specs = vec![
            $crate::machina_property_spec!($kind $name dynamic)
        ];
        specs.extend($crate::machina_property_specs![$($tail)*]);
        specs
    }};
    ($kind:ident $name:ident dynamic $(,)?) => {{
        vec![$crate::machina_property_spec!($kind $name dynamic)]
    }};
    ($kind:ident $name:ident, $($tail:tt)*) => {{
        let mut specs = vec![
            $crate::machina_property_spec!($kind $name)
        ];
        specs.extend($crate::machina_property_specs![$($tail)*]);
        specs
    }};
    ($kind:ident $name:ident $(,)?) => {{
        vec![$crate::machina_property_spec!($kind $name)]
    }};
}

#[macro_export]
macro_rules! machina_property_spec {
    (bool $name:ident) => {
        $crate::property::MPropertySpec::new(
            stringify!($name),
            $crate::property::MPropertyType::Bool,
        )
    };
    (bool $name:ident = $value:expr) => {
        $crate::machina_property_spec!(bool $name)
            .default($crate::property::MPropertyValue::Bool($value))
    };
    (bool $name:ident required) => {
        $crate::machina_property_spec!(bool $name).required()
    };
    (bool $name:ident dynamic) => {
        $crate::machina_property_spec!(bool $name).dynamic()
    };

    (u32 $name:ident) => {
        $crate::property::MPropertySpec::new(
            stringify!($name),
            $crate::property::MPropertyType::U32,
        )
    };
    (u32 $name:ident = $value:expr) => {
        $crate::machina_property_spec!(u32 $name)
            .default($crate::property::MPropertyValue::U32($value))
    };
    (u32 $name:ident required) => {
        $crate::machina_property_spec!(u32 $name).required()
    };
    (u32 $name:ident dynamic) => {
        $crate::machina_property_spec!(u32 $name).dynamic()
    };

    (u64 $name:ident) => {
        $crate::property::MPropertySpec::new(
            stringify!($name),
            $crate::property::MPropertyType::U64,
        )
    };
    (u64 $name:ident = $value:expr) => {
        $crate::machina_property_spec!(u64 $name)
            .default($crate::property::MPropertyValue::U64($value))
    };
    (u64 $name:ident required) => {
        $crate::machina_property_spec!(u64 $name).required()
    };
    (u64 $name:ident dynamic) => {
        $crate::machina_property_spec!(u64 $name).dynamic()
    };

    (string $name:ident) => {
        $crate::property::MPropertySpec::new(
            stringify!($name),
            $crate::property::MPropertyType::String,
        )
    };
    (string $name:ident = $value:expr) => {
        $crate::machina_property_spec!(string $name)
            .default($crate::property::MPropertyValue::String(String::from($value)))
    };
    (string $name:ident required) => {
        $crate::machina_property_spec!(string $name).required()
    };
    (string $name:ident dynamic) => {
        $crate::machina_property_spec!(string $name).dynamic()
    };

    (link $name:ident) => {
        $crate::property::MPropertySpec::new(
            stringify!($name),
            $crate::property::MPropertyType::Link,
        )
    };
    (link $name:ident = $value:expr) => {
        $crate::machina_property_spec!(link $name)
            .default($crate::property::MPropertyValue::Link(String::from($value)))
    };
    (link $name:ident required) => {
        $crate::machina_property_spec!(link $name).required()
    };
    (link $name:ident dynamic) => {
        $crate::machina_property_spec!(link $name).dynamic()
    };
}
