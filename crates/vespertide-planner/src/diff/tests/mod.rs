use super::*;
pub(super) use crate::test_support::{col_nullable as col, idx, table};
use rstest::rstest;
pub(super) use std::collections::BTreeSet;
pub(super) use vespertide_core::TableDef;
pub(super) use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, SimpleColumnType, TableConstraint,
    schema::{primary_key::PrimaryKeySyntax, str_or_bool::StrOrBoolOrArray},
};

mod basic;
mod column_changes;
mod constraint_performance;
mod constraint_removal;
mod coverage;
mod diff_tables;
mod enum_remap;
mod enums;
mod fk_ordering;
mod inline_constraints;
mod ordering_sort;
mod primary_key_changes;
