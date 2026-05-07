use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::mdev::MDevice;
use crate::property::MPropertySpec;

pub type MDeviceFactory = fn(&str) -> Box<dyn MDevice>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MObjectKind {
    Object,
    Device,
}

#[derive(Clone)]
pub struct MTypeInfo {
    name: String,
    kind: MObjectKind,
    parent_type: Option<String>,
    properties: Vec<MPropertySpec>,
    device_factory: Option<MDeviceFactory>,
}

impl MTypeInfo {
    pub fn object(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: MObjectKind::Object,
            parent_type: None,
            properties: Vec::new(),
            device_factory: None,
        }
    }

    pub fn device(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: MObjectKind::Device,
            parent_type: None,
            properties: Vec::new(),
            device_factory: None,
        }
    }

    pub fn with_parent(mut self, parent_type: &str) -> Self {
        self.parent_type = Some(parent_type.to_string());
        self
    }

    pub fn with_property(mut self, spec: MPropertySpec) -> Self {
        self.properties.push(spec);
        self
    }

    pub fn with_properties(mut self, specs: Vec<MPropertySpec>) -> Self {
        self.properties.extend(specs);
        self
    }

    pub fn with_device_factory(mut self, factory: MDeviceFactory) -> Self {
        self.device_factory = Some(factory);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> MObjectKind {
        self.kind
    }

    pub fn parent_type(&self) -> Option<&str> {
        self.parent_type.as_deref()
    }

    pub fn properties(&self) -> &[MPropertySpec] {
        &self.properties
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MTypeError {
    DuplicateType(String),
    MissingParentType(String),
    UnknownType(String),
    TypeIsNotDevice(String),
    MissingDeviceFactory(String),
    FactoryOnNonDevice(String),
    DuplicateProperty { type_name: String, property: String },
}

impl fmt::Display for MTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateType(name) => {
                write!(f, "type '{name}' is already registered")
            }
            Self::MissingParentType(name) => {
                write!(f, "parent type '{name}' is not registered")
            }
            Self::UnknownType(name) => {
                write!(f, "type '{name}' is not registered")
            }
            Self::TypeIsNotDevice(name) => {
                write!(f, "type '{name}' is not a device")
            }
            Self::MissingDeviceFactory(name) => {
                write!(f, "device type '{name}' has no factory")
            }
            Self::FactoryOnNonDevice(name) => {
                write!(
                    f,
                    "non-device type '{name}' must not have a device factory"
                )
            }
            Self::DuplicateProperty {
                type_name,
                property,
            } => {
                write!(
                    f,
                    "type '{type_name}' has duplicate property '{property}'"
                )
            }
        }
    }
}

impl std::error::Error for MTypeError {}

#[derive(Default)]
pub struct MTypeRegistry {
    types: BTreeMap<String, MTypeInfo>,
}

impl MTypeRegistry {
    pub fn register(&mut self, info: MTypeInfo) -> Result<(), MTypeError> {
        validate_type_info(&info)?;
        if self.types.contains_key(info.name()) {
            return Err(MTypeError::DuplicateType(info.name().to_string()));
        }
        self.types.insert(info.name.clone(), info);
        Ok(())
    }

    pub fn register_strict(
        &mut self,
        info: MTypeInfo,
    ) -> Result<(), MTypeError> {
        if let Some(parent_type) = info.parent_type() {
            if !self.types.contains_key(parent_type) {
                return Err(MTypeError::MissingParentType(
                    parent_type.to_string(),
                ));
            }
        }
        self.register(info)
    }

    pub fn get(&self, name: &str) -> Option<&MTypeInfo> {
        self.types.get(name)
    }

    pub fn create_device(
        &self,
        type_name: &str,
        local_id: &str,
    ) -> Result<Box<dyn MDevice>, MTypeError> {
        let info = self
            .get(type_name)
            .ok_or_else(|| MTypeError::UnknownType(type_name.to_string()))?;
        if info.kind != MObjectKind::Device {
            return Err(MTypeError::TypeIsNotDevice(type_name.to_string()));
        }
        let factory = info.device_factory.ok_or_else(|| {
            MTypeError::MissingDeviceFactory(type_name.to_string())
        })?;
        Ok(factory(local_id))
    }
}

fn validate_type_info(info: &MTypeInfo) -> Result<(), MTypeError> {
    if info.kind != MObjectKind::Device && info.device_factory.is_some() {
        return Err(MTypeError::FactoryOnNonDevice(info.name.clone()));
    }

    let mut names = BTreeSet::new();
    for property in &info.properties {
        if !names.insert(property.name.as_str()) {
            return Err(MTypeError::DuplicateProperty {
                type_name: info.name.clone(),
                property: property.name.clone(),
            });
        }
    }

    Ok(())
}
