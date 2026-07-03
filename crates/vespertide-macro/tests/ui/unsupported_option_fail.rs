use vespertide_macro::vespertide_migration;

fn main() {
    let db = ();
    let _ = vespertide_migration!(db, bad_option = "x");
}
