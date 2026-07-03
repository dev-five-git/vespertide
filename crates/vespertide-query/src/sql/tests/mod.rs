use super::*;
use crate::test_support::col;
use insta::{assert_snapshot, with_settings};
use rstest::rstest;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, ReferenceAction, SimpleColumnType, TableConstraint,
};

mod dispatch;
mod helpers;
mod naming;
