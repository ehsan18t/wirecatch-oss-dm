//! Fails a bridge test run that left out the `test-overrides` feature, which `bridge.rs` needs and would otherwise skip without a word.

#[cfg(not(feature = "test-overrides"))]
#[test]
fn the_bridge_tests_need_the_test_overrides_feature() {
    panic!(
        "the bridge's end-to-end tests did not run: use `cargo test -p wirecatch-bridge --features test-overrides`"
    );
}
