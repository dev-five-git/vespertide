#[test]
#[ignore = "trybuild UI tests require offline-locked compilation; run via `cargo test --test ui -- --include-ignored`"]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*_fail.rs");
}
