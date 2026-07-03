use vespertide_naming::build_enum_type_name;

#[test]
fn build_enum_type_name_prefixes_table_name_from_public_api() {
    assert_eq!(build_enum_type_name("invoice", "status"), "invoice_status");
}
