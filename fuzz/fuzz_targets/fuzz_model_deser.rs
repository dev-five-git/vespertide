//! Fuzz JSON deserialization of `TableDef`. Tests that any byte sequence
//! either returns `Ok(TableDef)` and survives basic consistency checks, or
//! returns `Err` — but never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(t) = serde_json::from_str::<vespertide_core::TableDef>(s) {
            // If parse succeeded, normalize must not panic.
            let _ = t.normalize();
            // Unique-column validation must not panic.
            let _ = t.validate_unique_column_names();
        }

        // Also fuzz MigrationPlan deserialization and basic action accessors.
        if let Ok(p) = serde_json::from_str::<vespertide_core::MigrationPlan>(s) {
            // Drive every action through `table_name()` without panicking.
            for action in &p.actions {
                let _ = action.table_name();
            }
        }
    }
});
