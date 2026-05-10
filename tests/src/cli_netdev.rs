use machina_core::machine::NetdevOpts;

#[test]
fn test_parse_netdev_valid() {
    let nd = NetdevOpts::parse("tap,id=net0,ifname=tap0", None).unwrap();
    assert_eq!(nd.id, "net0");
    assert_eq!(nd.ifname, "tap0");
    assert_eq!(nd.mac, None);
}

#[test]
fn test_parse_netdev_with_device_mac() {
    let nd = NetdevOpts::parse(
        "tap,id=net0,ifname=tap0",
        Some(
            "virtio-net-device,\
             netdev=net0,mac=52:54:00:12:34:56",
        ),
    )
    .unwrap();
    assert_eq!(nd.id, "net0");
    assert_eq!(nd.mac.as_deref(), Some("52:54:00:12:34:56"));
}

#[test]
fn test_parse_netdev_missing_id() {
    let result = NetdevOpts::parse("tap,ifname=tap0", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing id="));
}

#[test]
fn test_parse_netdev_missing_ifname() {
    let result = NetdevOpts::parse("tap,id=net0", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing ifname="));
}

#[test]
fn test_parse_netdev_unsupported_type() {
    let result = NetdevOpts::parse("user,id=net0", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unsupported type"));
}

#[test]
fn test_parse_device_without_netdev_field() {
    let result = NetdevOpts::parse(
        "tap,id=net0,ifname=tap0",
        Some(
            "virtio-net-device,\
             mac=52:54:00:12:34:56",
        ),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing netdev="));
}

#[test]
fn test_parse_device_netdev_id_mismatch() {
    let result = NetdevOpts::parse(
        "tap,id=net0,ifname=tap0",
        Some("virtio-net-device,netdev=other"),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("does not match"));
}

// ===== Empty-value and unsupported-device validation (#59) =====

#[test]
fn test_parse_netdev_empty_id_is_rejected() {
    let result = NetdevOpts::parse("tap,id=,ifname=tap0", None);
    let err = result.unwrap_err();
    assert!(
        err.contains("empty id"),
        "error message must name the empty id field, got: {err}",
    );
}

#[test]
fn test_parse_netdev_empty_ifname_is_rejected() {
    let result = NetdevOpts::parse("tap,id=net0,ifname=", None);
    let err = result.unwrap_err();
    assert!(
        err.contains("empty ifname"),
        "error message must name the empty ifname field, got: {err}",
    );
}

#[test]
fn test_parse_device_empty_netdev_is_rejected() {
    let result = NetdevOpts::parse(
        "tap,id=net0,ifname=tap0",
        Some("virtio-net-device,netdev="),
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("empty netdev"),
        "error must name the empty netdev= value, got: {err}",
    );
}

#[test]
fn test_parse_device_empty_mac_is_rejected() {
    let result = NetdevOpts::parse(
        "tap,id=net0,ifname=tap0",
        Some("virtio-net-device,netdev=net0,mac="),
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("empty mac"),
        "error must name the empty mac= value, got: {err}",
    );
}

#[test]
fn test_parse_device_unknown_kind_is_rejected() {
    // Unsupported -device kinds must be flagged here, not
    // silently accepted with their fields ignored.
    let result =
        NetdevOpts::parse("tap,id=net0,ifname=tap0", Some("e1000,netdev=net0"));
    let err = result.unwrap_err();
    assert!(
        err.contains("unsupported type") && err.contains("e1000"),
        "error must name the unsupported device kind, got: {err}",
    );
}

#[test]
fn test_parse_netdev_extra_commas_do_not_panic() {
    // Trailing/repeated separators are tolerated but the
    // required fields still need to be present.
    let nd = NetdevOpts::parse("tap,,id=net0,,ifname=tap0,", None).unwrap();
    assert_eq!(nd.id, "net0");
    assert_eq!(nd.ifname, "tap0");
}
