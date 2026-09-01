mod schemas;

pub(crate) use schemas::schema_scenario;
use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, DefaultValue, EnumValues, NumValue, ReferenceAction,
    SimpleColumnType, StrOrBoolOrArray, TableConstraint, TableDef,
};

pub(crate) fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, false)
}

fn nullable_col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

pub(super) fn simple(name: &str, ty: SimpleColumnType) -> ColumnDef {
    col(name, ColumnType::Simple(ty))
}

fn nullable_simple(name: &str, ty: SimpleColumnType) -> ColumnDef {
    nullable_col(name, ColumnType::Simple(ty))
}

pub(super) fn table(
    name: &str,
    columns: Vec<ColumnDef>,
    constraints: Vec<TableConstraint>,
) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

pub(super) fn pk(columns: &[&str]) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: columns.iter().copied().map(Into::into).collect(),
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}

fn auto_pk(columns: &[&str]) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: true,
        columns: columns.iter().copied().map(Into::into).collect(),
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}

pub(super) fn fk(columns: &[&str], ref_table: &str, ref_columns: &[&str]) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: None,
        columns: columns.iter().copied().map(Into::into).collect(),
        ref_table: ref_table.into(),
        ref_columns: ref_columns.iter().copied().map(Into::into).collect(),
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}

pub(crate) fn basic_table_with_description() -> TableDef {
    TableDef {
        name: "users".into(),
        description: Some("User accounts table".into()),
        columns: vec![
            simple("id", SimpleColumnType::Integer).comment("Primary key"),
            simple("email", SimpleColumnType::Text).comment("User email address"),
            nullable_simple("name", SimpleColumnType::Text),
        ],
        constraints: vec![
            auto_pk(&["id"]),
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    }
}

pub(crate) fn basic_single_pk() -> TableDef {
    table(
        "users",
        vec![
            simple("id", SimpleColumnType::Integer),
            nullable_simple("display_name", SimpleColumnType::Text),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn composite_pk() -> TableDef {
    table(
        "accounts",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("tenant_id", SimpleColumnType::BigInt),
        ],
        vec![pk(&["id", "tenant_id"])],
    )
}

pub(crate) fn table_with_fk() -> TableDef {
    table(
        "posts",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("user_id", SimpleColumnType::Integer),
            simple("title", SimpleColumnType::Text),
        ],
        vec![pk(&["id"]), fk(&["user_id"], "users", &["id"])],
    )
}

pub(crate) fn table_with_composite_fk() -> TableDef {
    table(
        "line_items",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("order_id", SimpleColumnType::Integer),
            simple("order_version", SimpleColumnType::Integer),
            simple("sku", SimpleColumnType::Text),
        ],
        vec![
            pk(&["id"]),
            fk(&["order_id", "order_version"], "orders", &["id", "version"]),
        ],
    )
}

/// A composite FK with both ends of the relation present. The referenced pair
/// is the target's composite PK, which is what makes the reference legal — and
/// what SeaORM and Prisma have to spell out on the relation while the Python
/// backends carry it as a table-level constraint.
pub(crate) fn composite_fk_relation() -> Vec<TableDef> {
    let orders = table(
        "orders",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("version", SimpleColumnType::Integer),
        ],
        vec![pk(&["id", "version"])],
    );
    vec![orders, table_with_composite_fk()]
}

pub(crate) fn inline_pk() -> TableDef {
    table(
        "users",
        vec![
            simple("id", SimpleColumnType::Uuid)
                .primary_key(PrimaryKeySyntax::Bool(true))
                .default("gen_random_uuid()".into()),
            simple("email", SimpleColumnType::Text).unique(StrOrBoolOrArray::Bool(true)),
        ],
        vec![],
    )
}

pub(crate) fn pk_and_fk_together() -> TableDef {
    let table = table(
        "article_user",
        vec![
            simple("article_id", SimpleColumnType::Uuid)
                .primary_key(PrimaryKeySyntax::Bool(true))
                .index(StrOrBoolOrArray::Bool(true))
                .foreign_key(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "article".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            simple("user_id", SimpleColumnType::Uuid)
                .primary_key(PrimaryKeySyntax::Bool(true))
                .index(StrOrBoolOrArray::Bool(true))
                .foreign_key(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            simple("author_order", SimpleColumnType::Integer).default("1".into()),
            col(
                "role",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 20 }),
            )
            .default("'contributor'".into()),
            simple("is_lead", SimpleColumnType::Boolean).default("false".into()),
            simple("created_at", SimpleColumnType::Timestamptz).default("now()".into()),
        ],
        vec![],
    );
    table.normalize().expect("pk/fk fixture normalizes")
}

pub(crate) fn table_with_enum() -> TableDef {
    table(
        "orders",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec![
                        "pending".into(),
                        "shipped".into(),
                        "delivered".into(),
                    ]),
                }),
            ),
        ],
        vec![],
    )
}

pub(crate) fn table_with_integer_enum() -> TableDef {
    table(
        "tasks",
        vec![
            simple("id", SimpleColumnType::Integer),
            col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "priority_level".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "medium".into(),
                            value: 10,
                        },
                        NumValue {
                            name: "high".into(),
                            value: 20,
                        },
                    ]),
                }),
            ),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn nullable_enum() -> TableDef {
    table(
        "nullable_enum",
        vec![
            simple("id", SimpleColumnType::Integer),
            nullable_col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status_type".into(),
                    values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                }),
            ),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn enum_multiple_columns() -> TableDef {
    table(
        "products",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "category",
                string_enum("product_category", &["electronics", "clothing", "food"]),
            ),
            col(
                "availability",
                string_enum(
                    "availability_status",
                    &["in_stock", "out_of_stock", "pre_order"],
                ),
            ),
        ],
        vec![],
    )
}

pub(crate) fn enum_shared() -> TableDef {
    table(
        "documents",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "status",
                string_enum("doc_status", &["draft", "published", "archived"]),
            ),
            nullable_col(
                "review_status",
                string_enum("doc_status", &["draft", "published", "archived"]),
            ),
        ],
        vec![],
    )
}

pub(crate) fn enum_special_values() -> TableDef {
    table(
        "events",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "severity",
                string_enum(
                    "event_severity",
                    &["info-level", "warning_level", "ERROR_LEVEL", "1critical"],
                ),
            ),
        ],
        vec![],
    )
}

fn string_enum(name: &str, values: &[&str]) -> ColumnType {
    ColumnType::Complex(ComplexColumnType::Enum {
        name: name.into(),
        values: EnumValues::String(values.iter().copied().map(Into::into).collect()),
    })
}

pub(crate) fn enum_with_default() -> TableDef {
    let mut table = table_with_enum();
    table.name = "tasks".into();
    table.columns[1].name = "status".into();
    table.columns[1].r#type = string_enum("task_status", &["pending", "in_progress", "completed"]);
    table.columns[1].default = Some("'pending'".into());
    table
        .columns
        .push(simple("priority", SimpleColumnType::Integer).default("0".into()));
    table
        .columns
        .push(simple("is_archived", SimpleColumnType::Boolean).default("false".into()));
    table
}

pub(crate) fn unique_and_indexed() -> TableDef {
    table(
        "users",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("email", SimpleColumnType::Text),
            simple("username", SimpleColumnType::Text),
            nullable_simple("department", SimpleColumnType::Text),
            simple("status", SimpleColumnType::Text).default("'active'".into()),
        ],
        vec![
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
            TableConstraint::Unique {
                name: Some("uq_username".into()),
                columns: vec!["username".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
            TableConstraint::Index {
                name: Some("idx_department".into()),
                columns: vec!["department".into()],
            },
        ],
    )
}

pub(crate) fn table_with_indexes() -> TableDef {
    table(
        "articles",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("title", SimpleColumnType::Text),
            simple("created_at", SimpleColumnType::Timestamptz),
        ],
        vec![
            pk(&["id"]),
            TableConstraint::Index {
                name: Some("idx_articles_created_at".into()),
                columns: vec!["created_at".into()],
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["title".into()],
            },
        ],
    )
}

pub(crate) fn table_level_pk() -> TableDef {
    table(
        "orders",
        vec![
            simple("id", SimpleColumnType::Uuid),
            simple("customer_id", SimpleColumnType::Uuid),
            simple("total", SimpleColumnType::Real),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn all_simple_types() -> TableDef {
    table(
        "all_types",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("small", SimpleColumnType::SmallInt),
            simple("big", SimpleColumnType::BigInt),
            simple("real_num", SimpleColumnType::Real),
            simple("double_num", SimpleColumnType::DoublePrecision),
            simple("text_col", SimpleColumnType::Text),
            simple("bool_col", SimpleColumnType::Boolean),
            simple("date_col", SimpleColumnType::Date),
            simple("time_col", SimpleColumnType::Time),
            simple("ts_col", SimpleColumnType::Timestamp),
            simple("tstz_col", SimpleColumnType::Timestamptz),
            simple("interval_col", SimpleColumnType::Interval),
            simple("bytea_col", SimpleColumnType::Bytea),
            simple("uuid_col", SimpleColumnType::Uuid),
            simple("json_col", SimpleColumnType::Json),
            simple("inet_col", SimpleColumnType::Inet),
            simple("cidr_col", SimpleColumnType::Cidr),
            simple("macaddr_col", SimpleColumnType::Macaddr),
            simple("xml_col", SimpleColumnType::Xml),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn complex_types() -> TableDef {
    table(
        "complex_types",
        vec![
            simple("id", SimpleColumnType::Integer),
            col(
                "varchar_col",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
            ),
            col(
                "char_col",
                ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
            ),
            col(
                "numeric_col",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            ),
            col(
                "custom_col",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "CUSTOM_TYPE".into(),
                }),
            ),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn jsonb_custom_type() -> TableDef {
    table(
        "json_struct",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("json_data", SimpleColumnType::Json),
            col(
                "jsonb_data",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "JSONB".into(),
                }),
            ),
            nullable_col(
                "jsonb_nullable",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "jsonb".into(),
                }),
            ),
        ],
        vec![],
    )
}

pub(crate) fn defaults() -> TableDef {
    table(
        "articles",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("published", SimpleColumnType::Boolean).default(DefaultValue::Bool(false)),
            simple("view_count", SimpleColumnType::Integer).default(DefaultValue::Integer(0)),
            simple("status", SimpleColumnType::Text)
                .default(DefaultValue::String("'draft'".into())),
        ],
        vec![auto_pk(&["id"])],
    )
}

pub(crate) fn server_defaults() -> TableDef {
    table(
        "with_defaults",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("created_at", SimpleColumnType::Timestamptz).default("now()".into()),
            simple("status", SimpleColumnType::Text).default("'active'".into()),
            simple("count", SimpleColumnType::Integer).default("0".into()),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn server_default_and_true_boolean() -> TableDef {
    table(
        "logs",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("active", SimpleColumnType::Boolean).default(DefaultValue::Bool(true)),
            simple("created_at", SimpleColumnType::Timestamptz)
                .default(DefaultValue::String("NOW()".into())),
            simple("score", SimpleColumnType::Real).default(DefaultValue::Float(1.5)),
            simple("tag", SimpleColumnType::Text)
                .default(DefaultValue::String("UNKNOWN_EXPR".into())),
        ],
        vec![auto_pk(&["id"])],
    )
}

pub(crate) fn nullable_columns() -> TableDef {
    table(
        "profiles",
        vec![
            simple("id", SimpleColumnType::Integer),
            nullable_simple("bio", SimpleColumnType::Text),
            nullable_col(
                "avatar_url",
                ColumnType::Complex(ComplexColumnType::Varchar { length: 500 }),
            ),
        ],
        vec![auto_pk(&["id"])],
    )
}

pub(crate) fn composite_constraints() -> TableDef {
    table(
        "order_items",
        vec![
            simple("order_id", SimpleColumnType::Integer),
            simple("product_id", SimpleColumnType::Integer),
            simple("quantity", SimpleColumnType::Integer),
        ],
        vec![
            pk(&["order_id", "product_id"]),
            fk(&["order_id"], "orders", &["id"]),
            fk(&["product_id"], "products", &["id"]),
            TableConstraint::Unique {
                name: Some("uq_order_items__order_product".into()),
                columns: vec!["order_id".into(), "product_id".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
            TableConstraint::Index {
                name: Some("ix_order_items__order_id".into()),
                columns: vec!["order_id".into()],
            },
        ],
    )
}

pub(crate) fn composite_unique() -> TableDef {
    table(
        "composite_unique",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("tenant_id", SimpleColumnType::Integer),
            simple("name", SimpleColumnType::Text),
        ],
        vec![
            pk(&["id"]),
            TableConstraint::Unique {
                name: Some("uq_tenant_name".into()),
                columns: vec!["tenant_id".into(), "name".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    )
}

pub(crate) fn composite_index() -> TableDef {
    table(
        "composite_index",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("tenant_id", SimpleColumnType::Integer),
            simple("name", SimpleColumnType::Text),
        ],
        vec![
            pk(&["id"]),
            TableConstraint::Index {
                name: Some("idx_tenant_name".into()),
                columns: vec!["tenant_id".into(), "name".into()],
            },
        ],
    )
}

pub(crate) fn unnamed_index_and_unique() -> TableDef {
    table(
        "events",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("venue_id", SimpleColumnType::Integer),
            simple("date", SimpleColumnType::Date),
        ],
        vec![
            pk(&["id"]),
            TableConstraint::Index {
                name: None,
                columns: vec!["venue_id".into(), "date".into()],
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["venue_id".into(), "date".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    )
}

pub(crate) fn unnamed_composite_index() -> TableDef {
    table(
        "unnamed_index",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("col_a", SimpleColumnType::Integer),
            simple("col_b", SimpleColumnType::Integer),
        ],
        vec![
            pk(&["id"]),
            TableConstraint::Index {
                name: None,
                columns: vec!["col_a".into(), "col_b".into()],
            },
        ],
    )
}

pub(crate) fn unnamed_composite_unique() -> TableDef {
    table(
        "unnamed_unique",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("col_a", SimpleColumnType::Integer),
            simple("col_b", SimpleColumnType::Integer),
        ],
        vec![
            pk(&["id"]),
            TableConstraint::Unique {
                name: None,
                columns: vec!["col_a".into(), "col_b".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    )
}

pub(crate) fn no_description() -> TableDef {
    table(
        "no_desc",
        vec![simple("id", SimpleColumnType::Integer)],
        vec![pk(&["id"])],
    )
}

pub(crate) fn string_default() -> TableDef {
    table(
        "string_defaults",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("status", SimpleColumnType::Text).default("'active'".into()),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn false_boolean_default() -> TableDef {
    table(
        "bool_defaults",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("is_deleted", SimpleColumnType::Boolean).default("false".into()),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn unknown_function_default() -> TableDef {
    table(
        "unknown_defaults",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("code", SimpleColumnType::Text).default("gen_code()".into()),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn unknown_constant_default() -> TableDef {
    table(
        "unknown_default",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("value", SimpleColumnType::Text).default("SOME_CONSTANT".into()),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn fk_with_comment_and_auto_increment() -> TableDef {
    table(
        "child",
        vec![
            simple("parent_id", SimpleColumnType::Integer).comment("References parent table"),
            simple("value", SimpleColumnType::Text),
        ],
        vec![
            auto_pk(&["parent_id"]),
            fk(&["parent_id"], "parent", &["id"]),
        ],
    )
}

pub(crate) fn json_default() -> TableDef {
    table(
        "configs",
        vec![
            simple("id", SimpleColumnType::Integer),
            simple("data", SimpleColumnType::Json).default(r#"{"hello": "world"}"#.into()),
        ],
        vec![pk(&["id"])],
    )
}

pub(crate) fn self_referencing_fk() -> TableDef {
    let raw = TableDef {
        name: "employees".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            nullable_simple("manager_id", SimpleColumnType::Integer).foreign_key(
                ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "employees".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::SetNull),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }),
            ),
        ],
        constraints: vec![],
    };
    raw.normalize().expect("self_referencing_fk normalizes")
}

pub(crate) fn reserved_word_identifiers() -> TableDef {
    let raw = TableDef {
        name: "order".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("user", SimpleColumnType::Text),
            simple("select", SimpleColumnType::Integer),
        ],
        constraints: vec![],
    };
    raw.normalize()
        .expect("reserved_word_identifiers normalizes")
}

/// Names that are legal in SQL — `quote_ident` quotes every identifier — but
/// that no target language accepts verbatim: a digit-leading table, a
/// digit-leading column and a hyphenated column. Each backend has to escape
/// them and name the original alongside.
pub(crate) fn non_identifier_names() -> TableDef {
    let raw = TableDef {
        name: "1users".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            nullable_simple("1st_place", SimpleColumnType::Integer),
            nullable_simple("user-id", SimpleColumnType::Text),
            // Self-referencing FK: the relation field names each backend derives
            // from this column and from the table name need escaping too.
            nullable_simple("1st_owner_id", SimpleColumnType::Integer),
        ],
        constraints: vec![fk(&["1st_owner_id"], "1users", &["id"])],
    };
    raw.normalize().expect("non_identifier_names normalizes")
}

/// Escape-needing names in the places a *model-level* constraint names them:
/// a composite primary key, a composite unique, and an index. Prisma spells
/// these with model field names (`@@id` / `@@unique` / `@@index`) while the
/// other backends spell them with database column names, so the same fixture
/// has to come out differently on each side.
pub(crate) fn non_identifier_names_in_constraints() -> TableDef {
    let raw = TableDef {
        name: "membership".into(),
        description: None,
        columns: vec![
            simple("1tenant_id", SimpleColumnType::Integer),
            simple("2user_id", SimpleColumnType::Integer),
            simple("user-email", SimpleColumnType::Text),
            simple("3created", SimpleColumnType::Text),
        ],
        constraints: vec![
            pk(&["1tenant_id", "2user_id"]),
            TableConstraint::Unique {
                name: None,
                columns: vec!["user-email".into(), "1tenant_id".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::default(),
            },
            TableConstraint::Index {
                name: None,
                columns: vec!["3created".into()],
            },
        ],
    };
    raw.normalize()
        .expect("non_identifier_names_in_constraints normalizes")
}

/// Two FK columns whose names differ only by the `_id` suffix, aimed at the
/// same table. Relation names derived from the stripped segment collapse onto
/// one string, so the backend has to keep the two relations apart or the
/// output declares the same relation twice.
pub(crate) fn fk_names_collide_after_id_strip() -> Vec<TableDef> {
    let target = TableDef {
        name: "target".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("alt", SimpleColumnType::Integer),
        ],
        constraints: vec![TableConstraint::Unique {
            name: None,
            columns: vec!["alt".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::default(),
        }],
    };
    let src = TableDef {
        name: "src".into(),
        description: None,
        columns: vec![
            simple("pk", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            nullable_simple("a_id", SimpleColumnType::Integer),
            nullable_simple("a", SimpleColumnType::Integer),
        ],
        constraints: vec![
            fk(&["a_id"], "target", &["id"]),
            fk(&["a"], "target", &["alt"]),
        ],
    };
    vec![
        target
            .normalize()
            .expect("fk_names_collide_after_id_strip target normalizes"),
        src.normalize()
            .expect("fk_names_collide_after_id_strip src normalizes"),
    ]
}

/// An FK column whose own name is what the relation field would be called, so
/// the relation and the column compete for one field name. Both have to end up
/// in the model, which means one of them gets renamed rather than redeclared.
pub(crate) fn relation_name_taken_by_column() -> Vec<TableDef> {
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
        ],
        constraints: vec![],
    };
    let items = TableDef {
        name: "items".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            nullable_simple("owner", SimpleColumnType::Integer),
        ],
        constraints: vec![fk(&["owner"], "users", &["id"])],
    };
    vec![
        users
            .normalize()
            .expect("relation_name_taken_by_column users normalizes"),
        items
            .normalize()
            .expect("relation_name_taken_by_column items normalizes"),
    ]
}

/// Two tables where every name a relation is derived from needs escaping: the
/// referenced table, its primary key, and both referencing columns. Two FKs to
/// the same table is what forces the disambiguating names — the reverse
/// `has_many` field, `relation_enum` / `via_rel`, `from` / `to`, and the
/// module path — so each one is exercised on a name its language rejects.
pub(crate) fn non_identifier_relation_names() -> Vec<TableDef> {
    let owners = TableDef {
        name: "1users".into(),
        description: None,
        columns: vec![
            simple("1id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("email", SimpleColumnType::Text),
        ],
        constraints: vec![],
    };
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            nullable_simple("1st_owner_id", SimpleColumnType::Integer),
            nullable_simple("2nd_owner_id", SimpleColumnType::Integer),
        ],
        constraints: vec![
            fk(&["1st_owner_id"], "1users", &["1id"]),
            fk(&["2nd_owner_id"], "1users", &["1id"]),
        ],
    };
    vec![
        owners
            .normalize()
            .expect("non_identifier_relation_names owners normalizes"),
        posts
            .normalize()
            .expect("non_identifier_relation_names posts normalizes"),
    ]
}

pub(crate) fn composite_primary_key() -> TableDef {
    let raw = TableDef {
        name: "membership".into(),
        description: None,
        columns: vec![
            simple("tenant_id", SimpleColumnType::Integer)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            simple("user_id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("role", SimpleColumnType::Text),
        ],
        constraints: vec![],
    };
    raw.normalize().expect("composite_primary_key normalizes")
}

pub(crate) fn composite_unique_constraint() -> TableDef {
    let raw = TableDef {
        name: "account_aliases".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            simple("tenant_id", SimpleColumnType::Integer).unique(StrOrBoolOrArray::Array(vec![
                "uq_account_aliases__tenant_slug".into(),
            ])),
            simple("slug", SimpleColumnType::Text).unique(StrOrBoolOrArray::Array(vec![
                "uq_account_aliases__tenant_slug".into(),
            ])),
        ],
        constraints: vec![],
    };
    raw.normalize()
        .expect("composite_unique_constraint normalizes")
}

pub(crate) fn numeric_default_value() -> TableDef {
    table(
        "products",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "price",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
            )
            .default(DefaultValue::Float(0.0)),
        ],
        vec![],
    )
}

pub(crate) fn integer_enum_with_default() -> TableDef {
    table(
        "tasks",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Completed".into(),
                            value: 100,
                        },
                    ]),
                }),
            )
            .default("1".into()),
        ],
        vec![],
    )
}

/// Integer enum column whose `default` is a **variant name** (not a numeric
/// literal), exercising the `else` branch inside the
/// `EnumValues::Integer(int_values) =>` arm of
/// `vespertide-exporter::seaorm::types::format_default_value` (lines 47-60
/// of `seaorm/types.rs`). Existing `integer_enum_with_default` covers the
/// numeric-literal `if` branch via `default("1")`; this fixture covers the
/// variant-name lookup branch via `default("Completed")` → value `100`.
pub(crate) fn integer_enum_with_variant_default() -> TableDef {
    table(
        "task_runs",
        vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_run_status".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "Pending".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "Completed".into(),
                            value: 100,
                        },
                    ]),
                }),
            )
            .default("Completed".into()),
        ],
        vec![],
    )
}

/// Small (< 50-table) multi-table schema for exercising the **sequential**
/// branch of the multi-table entry points:
/// * `vespertide-exporter::sqlalchemy::render::export` lines 21-29
/// * `vespertide-exporter::sqlmodel::render::render_entities` lines 62-65, 94-100
/// * `vespertide-exporter::seaorm::export` sequential arm
/// * `vespertide-exporter::jpa::render_entities` sequential arm
///
/// The existing `tests/parallel_consolidated.rs` test uses a 100-table schema
/// to exercise the parallel arm; this fixture covers the sequential arm.
pub(crate) fn small_multi_schema() -> Vec<TableDef> {
    vec![basic_single_pk(), table_with_fk()]
}

pub(crate) fn integer_enum_all_variant_types() -> TableDef {
    let raw = TableDef {
        name: "workflow_runs".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer).primary_key(PrimaryKeySyntax::Bool(true)),
            col(
                "state",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "edge_state".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "unknown".into(),
                            value: -1,
                        },
                        NumValue {
                            name: "not_started".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "InProgress".into(),
                            value: 10,
                        },
                        NumValue {
                            name: "HTTP_500".into(),
                            value: 500,
                        },
                    ]),
                }),
            ),
        ],
        constraints: vec![],
    };
    raw.normalize()
        .expect("integer_enum_all_variant_types normalizes")
}
