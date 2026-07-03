use std::collections::{HashMap, HashSet};

/// Parse `fill_with` arguments from CLI.
/// Format: table.column=value
pub(super) fn parse_fill_with_args(args: &[String]) -> HashMap<(String, String), String> {
    let mut map = HashMap::new();
    for arg in args {
        if let Some((key, value)) = arg.split_once('=')
            && let Some((table, column)) = key.split_once('.')
        {
            map.insert((table.to_string(), column.to_string()), value.to_string());
        }
    }
    map
}

/// Parse `delete_null_rows` arguments from CLI.
/// Format: table.column
pub(super) fn parse_delete_null_rows_args(args: &[String]) -> HashSet<(String, String)> {
    let mut set = HashSet::new();
    for arg in args {
        if let Some((table, column)) = arg.split_once('.') {
            set.insert((table.to_string(), column.to_string()));
        }
    }
    set
}
