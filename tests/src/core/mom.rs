use std::any::Any;

use machina_core::mobject::{MObject, MObjectState, MObjectTree};

struct TestObject {
    state: MObjectState,
}

impl TestObject {
    fn new_root(local_id: &str) -> Self {
        Self {
            state: MObjectState::new_root(local_id).expect("valid root object"),
        }
    }

    fn new_detached(local_id: &str) -> Self {
        Self {
            state: MObjectState::new_detached(local_id)
                .expect("valid detached object"),
        }
    }
}

impl MObject for TestObject {
    fn mobject_state(&self) -> &MObjectState {
        &self.state
    }

    fn mobject_state_mut(&mut self) -> &mut MObjectState {
        &mut self.state
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn test_mobject_root_path() {
    let root = TestObject::new_root("machine");
    assert_eq!(root.local_id(), "machine");
    assert_eq!(root.object_path(), Some("/machine"));
    assert!(root.parent_path().is_none());
    assert!(root.child_paths().is_empty());
}

#[test]
fn test_mobject_attach_child_sets_parent_and_path() {
    let mut root = TestObject::new_root("machine");
    let mut child = TestObject::new_detached("uart0");

    root.mobject_state_mut()
        .attach_child(child.mobject_state_mut())
        .expect("attach child");

    assert_eq!(child.parent_path(), Some("/machine"));
    assert_eq!(child.object_path(), Some("/machine/uart0"));
    assert_eq!(root.child_paths(), &["/machine/uart0".to_string()]);
}

#[test]
fn test_mobject_duplicate_child_id_rejected() {
    let mut root = TestObject::new_root("machine");
    let mut child1 = TestObject::new_detached("uart0");
    let mut child2 = TestObject::new_detached("uart0");

    root.mobject_state_mut()
        .attach_child(child1.mobject_state_mut())
        .expect("attach first child");
    let err = root
        .mobject_state_mut()
        .attach_child(child2.mobject_state_mut())
        .expect_err("duplicate child id must fail");
    assert_eq!(
        err.to_string(),
        "child local_id 'uart0' is already attached under parent"
    );
}

#[test]
fn test_mobject_detached_parent_cannot_attach_child() {
    let mut parent = TestObject::new_detached("parent");
    let mut child = TestObject::new_detached("uart0");

    let err = parent
        .mobject_state_mut()
        .attach_child(child.mobject_state_mut())
        .expect_err("detached parent must not attach a child");

    assert_eq!(err, machina_core::mobject::MObjectError::ParentDetached);
    assert!(child.object_path().is_none());
}

#[test]
fn test_mobject_detach_unrelated_child_is_rejected() {
    let mut root = TestObject::new_root("machine");
    let mut child = TestObject::new_detached("uart0");

    let err = root
        .mobject_state_mut()
        .detach_child(child.mobject_state_mut())
        .expect_err("unrelated child detach must fail");

    assert_eq!(err, machina_core::mobject::MObjectError::ChildPathMismatch);
}

#[test]
fn test_mobject_detach_child_clears_attachment() {
    let mut root = TestObject::new_root("machine");
    let mut child = TestObject::new_detached("uart0");

    root.mobject_state_mut()
        .attach_child(child.mobject_state_mut())
        .expect("attach child");
    root.mobject_state_mut()
        .detach_child(child.mobject_state_mut())
        .expect("detach child");

    assert!(root.child_paths().is_empty());
    assert!(child.parent_path().is_none());
    assert!(child.object_path().is_none());
}

#[test]
fn test_mobject_compile_time_type_check() {
    let obj = TestObject::new_root("machine");
    assert!(obj.is_type::<TestObject>());
}

#[test]
fn test_mobject_tree_tracks_attach_lookup_and_detach() {
    let mut root = TestObject::new_root("machine");
    let mut child = TestObject::new_detached("uart0");
    let mut tree = MObjectTree::default();

    tree.track_root(root.mobject_state())
        .expect("track root object");
    tree.attach_child(root.mobject_state_mut(), child.mobject_state_mut())
        .expect("attach child through tree");

    assert_eq!(tree.lookup("/machine").unwrap().local_id, "machine");
    assert_eq!(tree.lookup("/machine/uart0").unwrap().local_id, "uart0");
    assert_eq!(
        tree.lookup("/machine").unwrap().child_paths,
        vec!["/machine/uart0".to_string()]
    );

    tree.detach_child(root.mobject_state_mut(), child.mobject_state_mut())
        .expect("detach child through tree");

    assert!(tree.lookup("/machine/uart0").is_none());
    assert!(tree.lookup("/machine").unwrap().child_paths.is_empty());
}

#[test]
fn test_mobject_tree_rejects_non_leaf_detach() {
    let mut root = TestObject::new_root("machine");
    let mut bus = TestObject::new_detached("bus0");
    let mut device = TestObject::new_detached("dev0");
    let mut tree = MObjectTree::default();

    tree.track_root(root.mobject_state())
        .expect("track root object");
    tree.attach_child(root.mobject_state_mut(), bus.mobject_state_mut())
        .expect("attach bus through tree");
    tree.attach_child(bus.mobject_state_mut(), device.mobject_state_mut())
        .expect("attach device through tree");

    assert!(tree.lookup("/machine/bus0").is_some());
    assert!(tree.lookup("/machine/bus0/dev0").is_some());

    let err = tree
        .detach_child(root.mobject_state_mut(), bus.mobject_state_mut())
        .expect_err("non-leaf detach must fail");
    assert_eq!(err, machina_core::mobject::MObjectError::ChildHasChildren);
    assert_eq!(bus.object_path(), Some("/machine/bus0"));
    assert_eq!(device.object_path(), Some("/machine/bus0/dev0"));
    assert!(tree.lookup("/machine/bus0").is_some());
    assert!(tree.lookup("/machine/bus0/dev0").is_some());

    tree.detach_child(bus.mobject_state_mut(), device.mobject_state_mut())
        .expect("detach leaf device through tree");
    assert!(tree.lookup("/machine/bus0/dev0").is_none());
    assert!(device.object_path().is_none());

    tree.detach_child(root.mobject_state_mut(), bus.mobject_state_mut())
        .expect("detach leaf bus through tree");
    assert!(tree.lookup("/machine/bus0").is_none());
    assert!(tree.lookup("/machine").unwrap().child_paths.is_empty());
    assert!(bus.object_path().is_none());
}
