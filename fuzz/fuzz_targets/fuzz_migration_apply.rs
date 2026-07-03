//! Fuzz migration application from JSON inputs. Invalid bytes or invalid JSON
//! are ignored; valid schema/action pairs must never panic and should only
//! report invalid migrations through `Result::Err`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vespertide_planner::apply_action;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // First half: JSON array of TableDef. Second half: JSON MigrationAction.
        let Some((schema_json, action_json)) = s.split_once("\n----\n") else {
            return;
        };

        let Ok(mut schema) = serde_json::from_str::<Vec<vespertide_core::TableDef>>(schema_json)
        else {
            return;
        };
        let Ok(action) = serde_json::from_str::<vespertide_core::MigrationAction>(action_json)
        else {
            return;
        };

        let _ = apply_action(&mut schema, &action);
    }
});
