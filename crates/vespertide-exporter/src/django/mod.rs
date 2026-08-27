mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_config::DjangoConfig;
use vespertide_core::TableDef;

pub use render::{
    export, export_with_config, render_entity, render_entity_with_schema,
    render_entity_with_schema_and_config,
};

pub struct DjangoExporter;

impl OrmExporter for DjangoExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema(table, schema)
    }
}

/// Django exporter that honors `vespertide.json`'s `django` config section
/// (currently an optional `app_label` written into every model's `Meta`
/// class). Mirrors `seaorm::SeaOrmExporterWithConfig`.
pub struct DjangoExporterWithConfig<'a> {
    pub config: &'a DjangoConfig,
}

impl<'a> DjangoExporterWithConfig<'a> {
    pub fn new(config: &'a DjangoConfig) -> Self {
        Self { config }
    }

    pub fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema_and_config(table, schema, self.config.app_label())
    }
}

#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    render::to_pascal_case(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use rstest::rstest;
    use vespertide_core::schema::column::{EnumValues, SimpleColumnType};
    use vespertide_core::schema::constraint::TableConstraint;
    use vespertide_core::{
        ColumnType, ComplexColumnType, DefaultValue, NumValue, ReferenceAction, TableDef,
    };

    fn col(name: &str, ty: ColumnType) -> vespertide_core::ColumnDef {
        vespertide_core::ColumnDef::new(name, ty, false)
    }

    fn nullable_col(name: &str, ty: ColumnType) -> vespertide_core::ColumnDef {
        vespertide_core::ColumnDef::new(name, ty, true)
    }

    fn auto_pk(columns: &[&str]) -> TableConstraint {
        TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }
    }

    fn pk(columns: &[&str]) -> TableConstraint {
        TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }
    }

    fn fk(col: &str, ref_table: &str, on_delete: Option<ReferenceAction>) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: None,
            columns: vec![col.into()],
            ref_table: ref_table.into(),
            ref_columns: vec!["id".into()],
            on_delete,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Basic table with autoincrement PK + nullable field
    // -----------------------------------------------------------------------

    #[test]
    fn test_basic_table() {
        let table = TableDef {
            name: "users".into(),
            description: Some("User accounts".into()),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "email",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
                ),
                nullable_col("name", ColumnType::Simple(SimpleColumnType::Text)),
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
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // FK field: `_id` suffix stripping
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_fk() {
        let table = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                fk("author_id", "users", Some(ReferenceAction::Cascade)),
            ],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // TextChoices enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_string_enum() {
        let table = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                let mut c = col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                            "delivered".into(),
                        ]),
                    }),
                );
                c.default = Some(DefaultValue::String("'pending'".into()));
                c
            }],
            constraints: vec![auto_pk(&["id"])],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // IntegerChoices enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_with_integer_enum() {
        let table = TableDef {
            name: "tasks".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
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
            constraints: vec![pk(&["id"])],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // Composite PK
    // -----------------------------------------------------------------------

    #[test]
    fn test_composite_pk() {
        let table = TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("quantity", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![pk(&["order_id", "product_id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains("pk = models.CompositePrimaryKey(\"order_id\", \"product_id\")"),
            "expected Django 5.2+ CompositePrimaryKey declaration, got:\n{result}"
        );
        assert!(
            !result.contains("primary_key=True"),
            "individual composite-PK columns must not also carry primary_key=True, got:\n{result}"
        );
        assert_snapshot!(result);
    }

    #[test]
    fn test_composite_pk_of_fk_columns_uses_attname_not_field_name() {
        // Composite PK made of FK columns: CompositePrimaryKey must reference
        // the Django attname ("{field}_id"), not the stripped field name
        // ("article"/"user") used for the ForeignKey attribute itself.
        let table = TableDef {
            name: "article_user".into(),
            description: None,
            columns: vec![
                col("article_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                pk(&["article_id", "user_id"]),
                fk("article_id", "articles", Some(ReferenceAction::Cascade)),
                fk("user_id", "users", Some(ReferenceAction::Cascade)),
            ],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains("pk = models.CompositePrimaryKey(\"article_id\", \"user_id\")"),
            "expected attname-based CompositePrimaryKey args, got:\n{result}"
        );
    }

    // -----------------------------------------------------------------------
    // Indexes and composite unique in Meta
    // -----------------------------------------------------------------------

    #[test]
    fn test_indexes_and_composite_unique() {
        let table = TableDef {
            name: "articles".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "slug",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 200 }),
                ),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                ),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                TableConstraint::Index {
                    name: Some("ix_articles__created_at".into()),
                    columns: vec!["created_at".into()],
                },
                TableConstraint::Unique {
                    name: Some("uq_articles__slug_author".into()),
                    columns: vec!["slug".into(), "author_id".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                        keep: vespertide_core::KeepPolicy::First,
                    },
                },
            ],
        };
        assert_snapshot!(render_entity(&table).unwrap());
    }

    // -----------------------------------------------------------------------
    // server default (NOW()) → timezone.now
    // -----------------------------------------------------------------------

    #[test]
    fn test_server_default_timezone() {
        let table = TableDef {
            name: "events".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                {
                    let mut c = col(
                        "created_at",
                        ColumnType::Simple(SimpleColumnType::Timestamptz),
                    );
                    c.default = Some(DefaultValue::String("NOW()".into()));
                    c
                },
                {
                    let mut c = col("count", ColumnType::Simple(SimpleColumnType::Integer));
                    c.default = Some(DefaultValue::Integer(0));
                    c
                },
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(result.contains("from django.utils import timezone"));
        assert!(result.contains("default=timezone.now"));
        assert!(result.contains("default=0"));
        assert_snapshot!(result);
    }

    // -----------------------------------------------------------------------
    // Type coverage — all simple types
    // -----------------------------------------------------------------------

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "models.SmallIntegerField")]
    #[case::bigint(SimpleColumnType::BigInt, "models.BigIntegerField")]
    #[case::real(SimpleColumnType::Real, "models.FloatField")]
    #[case::text(SimpleColumnType::Text, "models.TextField")]
    #[case::boolean(SimpleColumnType::Boolean, "models.BooleanField")]
    #[case::date(SimpleColumnType::Date, "models.DateField")]
    #[case::time(SimpleColumnType::Time, "models.TimeField")]
    #[case::timestamp(SimpleColumnType::Timestamp, "models.DateTimeField")]
    #[case::uuid(SimpleColumnType::Uuid, "models.UUIDField")]
    #[case::json(SimpleColumnType::Json, "models.JSONField")]
    #[case::bytea(SimpleColumnType::Bytea, "models.BinaryField")]
    #[case::inet(SimpleColumnType::Inet, "models.GenericIPAddressField")]
    #[case::interval(SimpleColumnType::Interval, "models.DurationField")]
    #[case::macaddr(SimpleColumnType::Macaddr, "models.CharField")]
    fn test_simple_type_mapping(#[case] ty: SimpleColumnType, #[case] expected: &str) {
        let table = TableDef {
            name: "t".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("val", ColumnType::Simple(ty)),
            ],
            constraints: vec![pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(expected),
            "expected {expected} in:\n{result}"
        );
    }

    #[rstest]
    #[case::small_auto(SimpleColumnType::SmallInt, "models.SmallAutoField")]
    #[case::big_auto(SimpleColumnType::BigInt, "models.BigAutoField")]
    fn test_auto_pk_field_types(#[case] ty: SimpleColumnType, #[case] expected: &str) {
        let table = TableDef {
            name: "t".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(ty))],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(expected),
            "expected {expected} in:\n{result}"
        );
    }

    #[test]
    fn test_numeric_field() {
        let table = TableDef {
            name: "prices".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "amount",
                    ColumnType::Complex(ComplexColumnType::Numeric {
                        precision: 10,
                        scale: 2,
                    }),
                ),
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains("models.DecimalField"),
            "expected DecimalField"
        );
        assert!(result.contains("max_digits=10"), "expected max_digits=10");
        assert!(
            result.contains("decimal_places=2"),
            "expected decimal_places=2"
        );
    }

    #[test]
    fn test_custom_type_field() {
        let table = TableDef {
            name: "docs".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "data",
                    ColumnType::Complex(ComplexColumnType::Custom {
                        custom_type: "JSONB".into(),
                    }),
                ),
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        // Custom type → models.TextField (Django has no native JSONB)
        assert!(
            result.contains("data = models.TextField()"),
            "expected Custom→TextField in:\n{result}"
        );
    }

    #[test]
    fn test_uuid_default() {
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Uuid));
        id_col.default = Some(DefaultValue::String("gen_random_uuid()".into()));
        let table = TableDef {
            name: "sessions".into(),
            description: None,
            columns: vec![id_col],
            constraints: vec![pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(result.contains("import uuid"), "expected uuid import");
        assert!(
            result.contains("default=uuid.uuid4"),
            "expected uuid4 callable"
        );
    }

    #[test]
    fn test_export_multi_table() {
        let users = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![auto_pk(&["id"])],
        };
        let posts = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                fk("author_id", "users", Some(ReferenceAction::Cascade)),
            ],
        };
        let result = export(&[users, posts]).unwrap();
        assert!(result.contains("class Users(models.Model):"));
        assert!(result.contains("class Posts(models.Model):"));
    }

    #[test]
    fn test_nullable_fk_with_db_column() {
        // FK column without `_id` suffix → emits db_column kwarg
        // Nullable FK → emits null=True, blank=True
        let table = TableDef {
            name: "comments".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                nullable_col("parent", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                fk("parent", "comments", Some(ReferenceAction::SetNull)),
            ],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(r#"db_column="parent""#),
            "expected db_column kwarg"
        );
        assert!(
            result.contains("null=True"),
            "expected null=True for nullable FK"
        );
        assert!(
            result.contains("blank=True"),
            "expected blank=True for nullable FK"
        );
    }

    // -----------------------------------------------------------------------
    // build_default: Boolean false → "False"
    // -----------------------------------------------------------------------

    #[test]
    fn test_bool_false_default() {
        let mut flag = col("enabled", ColumnType::Simple(SimpleColumnType::Boolean));
        flag.default = Some(DefaultValue::Bool(false));
        let table = TableDef {
            name: "settings".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                flag,
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(result.contains("default=False"), "expected default=False");
    }

    // -----------------------------------------------------------------------
    // build_default: Boolean true → "True"
    // -----------------------------------------------------------------------

    #[test]
    fn test_bool_true_default() {
        let mut flag = col("enabled", ColumnType::Simple(SimpleColumnType::Boolean));
        flag.default = Some(DefaultValue::Bool(true));
        let table = TableDef {
            name: "settings".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                flag,
            ],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(result.contains("default=True"), "expected default=True");
    }

    // -----------------------------------------------------------------------
    // build_default: functional default on non-Timestamp/UUID type → None (omitted)
    // -----------------------------------------------------------------------

    #[test]
    fn test_functional_default_non_special() {
        let mut seq_id = col("seq_id", ColumnType::Simple(SimpleColumnType::Integer));
        seq_id.default = Some(DefaultValue::String("nextval('my_seq')".into()));
        let table = TableDef {
            name: "items".into(),
            description: None,
            columns: vec![seq_id],
            constraints: vec![pk(&["seq_id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            !result.contains("default="),
            "functional default should be omitted"
        );
    }

    // -----------------------------------------------------------------------
    // reference_action_str: Restrict, SetDefault, NoAction
    // -----------------------------------------------------------------------

    #[rstest]
    #[case(ReferenceAction::Restrict, "models.RESTRICT")]
    #[case(ReferenceAction::SetDefault, "models.SET_DEFAULT")]
    #[case(ReferenceAction::NoAction, "models.DO_NOTHING")]
    fn test_fk_on_delete_actions(#[case] action: ReferenceAction, #[case] expected: &str) {
        let table = TableDef {
            name: "comments".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("post_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![auto_pk(&["id"]), fk("post_id", "posts", Some(action))],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(expected),
            "expected {expected} in:\n{result}"
        );
    }

    // -----------------------------------------------------------------------
    // Column comment → emits "# ..." line before the field
    // -----------------------------------------------------------------------

    #[test]
    fn test_column_comment() {
        let mut c = col("name", ColumnType::Simple(SimpleColumnType::Text));
        c.comment = Some("The user's full name".into());
        let table = TableDef {
            name: "users".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), c],
            constraints: vec![auto_pk(&["id"])],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains("    # The user's full name"),
            "expected column comment in output"
        );
    }

    // -----------------------------------------------------------------------
    // Unnamed index and unnamed composite unique in Meta
    // -----------------------------------------------------------------------

    #[test]
    fn test_index_and_unique_no_name() {
        let table = TableDef {
            name: "entries".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "slug",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
                ),
                col(
                    "tag",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
                ),
            ],
            constraints: vec![
                auto_pk(&["id"]),
                TableConstraint::Index {
                    name: None,
                    columns: vec!["slug".into()],
                },
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["slug".into(), "tag".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                        keep: vespertide_core::KeepPolicy::First,
                    },
                },
            ],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains("models.Index(fields=[\"slug\"]),"),
            "expected unnamed Index"
        );
        assert!(
            result.contains("models.UniqueConstraint(fields=[\"slug\", \"tag\"]),"),
            "expected unnamed UniqueConstraint"
        );
    }

    // -----------------------------------------------------------------------
    // Many-to-many junction table recognition (render_entity_with_schema)
    // -----------------------------------------------------------------------

    fn users_table() -> TableDef {
        TableDef {
            name: "users".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![auto_pk(&["id"])],
        }
    }

    fn tags_table() -> TableDef {
        TableDef {
            name: "tags".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![auto_pk(&["id"])],
        }
    }

    fn junction_table(
        name: &str,
        left_col: &str,
        left_ref: &str,
        right_col: &str,
        right_ref: &str,
    ) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: vec![
                col(left_col, ColumnType::Simple(SimpleColumnType::Integer)),
                col(right_col, ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                pk(&[left_col, right_col]),
                fk(left_col, left_ref, None),
                fk(right_col, right_ref, None),
            ],
        }
    }

    #[test]
    fn test_many_to_many_junction_table() {
        let users = users_table();
        let tags = tags_table();
        let user_tags = junction_table("user_tags", "user_id", "users", "tag_id", "tags");
        let schema = vec![users.clone(), tags.clone(), user_tags.clone()];

        let result = render_entity_with_schema(&users, &schema).unwrap();
        assert!(
            result.contains(
                "tags = models.ManyToManyField(\"Tags\", through=\"UserTags\", related_name=\"+\")"
            ),
            "expected ManyToManyField on users side, got:\n{result}"
        );

        let result = render_entity_with_schema(&tags, &schema).unwrap();
        assert!(
            result.contains(
                "users = models.ManyToManyField(\"Users\", through=\"UserTags\", related_name=\"+\")"
            ),
            "expected ManyToManyField on tags side, got:\n{result}"
        );
    }

    #[test]
    fn test_many_to_many_disambiguates_multiple_junctions_to_same_target() {
        let users = users_table();
        let tags = tags_table();
        let user_tags = junction_table("user_tags", "user_id", "users", "tag_id", "tags");
        let user_favorite_tags =
            junction_table("user_favorite_tags", "user_id", "users", "tag_id", "tags");
        let schema = vec![users.clone(), tags, user_tags, user_favorite_tags];

        let result = render_entity_with_schema(&users, &schema).unwrap();
        assert!(
            result.contains(
                "tags_via_user_tags = models.ManyToManyField(\"Tags\", through=\"UserTags\""
            ),
            "expected disambiguated field for user_tags junction, got:\n{result}"
        );
        assert!(
            result.contains(
                "tags_via_user_favorite_tags = models.ManyToManyField(\"Tags\", through=\"UserFavoriteTags\""
            ),
            "expected disambiguated field for user_favorite_tags junction, got:\n{result}"
        );
    }

    #[test]
    fn test_purely_self_referential_junction_is_skipped() {
        // "friends" links users to users on both sides — not a two-sided M2M
        // we can safely name, so no ManyToManyField should be emitted.
        let users = users_table();
        let friends = junction_table("friends", "user_id", "users", "friend_id", "users");
        let schema = vec![users.clone(), friends];

        let result = render_entity_with_schema(&users, &schema).unwrap();
        assert!(
            !result.contains("ManyToManyField"),
            "self-referential junction must not produce a guessed M2M field, got:\n{result}"
        );
    }

    #[test]
    fn test_junction_table_unrelated_to_current_table_is_ignored() {
        // "order_tags" is a genuine junction (composite PK, 2 FKs both in the
        // PK), but neither side references `users` at all — it links
        // "orders" and "tags" together, so it must not produce any
        // ManyToManyField on `users`.
        let users = users_table();
        let tags = tags_table();
        let orders = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![auto_pk(&["id"])],
        };
        let order_tags = junction_table("order_tags", "order_id", "orders", "tag_id", "tags");
        let schema = vec![users.clone(), tags, orders, order_tags];

        let result = render_entity_with_schema(&users, &schema).unwrap();
        assert!(
            !result.contains("ManyToManyField"),
            "junction table unrelated to `users` must not produce a M2M field, got:\n{result}"
        );
    }

    #[test]
    fn test_export_multi_table_includes_many_to_many() {
        let users = users_table();
        let tags = tags_table();
        let user_tags = junction_table("user_tags", "user_id", "users", "tag_id", "tags");
        let result = export(&[users, tags, user_tags]).unwrap();
        assert!(
            result.contains("models.ManyToManyField(\"Tags\", through=\"UserTags\""),
            "expected ManyToManyField in multi-table export, got:\n{result}"
        );
    }

    // -----------------------------------------------------------------------
    // Composite FK: Django has no native multi-column FK field, so it must
    // be surfaced as a comment instead of silently dropped.
    // -----------------------------------------------------------------------

    #[test]
    fn test_composite_fk_emits_comment() {
        let table = TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                pk(&["order_id", "region_id"]),
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["order_id".into(), "region_id".into()],
                    ref_table: "order_regions".into(),
                    ref_columns: vec!["order_id".into(), "region_id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            ],
        };
        let result = render_entity(&table).unwrap();
        assert!(
            result.contains(
                "# composite foreign key: (order_id, region_id) -> order_regions(order_id, region_id)"
            ),
            "expected composite FK comment, got:\n{result}"
        );
    }

    // -----------------------------------------------------------------------
    // DjangoExporterWithConfig: app_label reaches the Meta class
    // -----------------------------------------------------------------------

    #[test]
    fn test_app_label_omitted_by_default() {
        let table = users_table();
        let schema = vec![table.clone()];
        let config = DjangoConfig::default();
        let exporter = DjangoExporterWithConfig::new(&config);
        let result = exporter.render_entity_with_schema(&table, &schema).unwrap();
        assert!(
            !result.contains("app_label"),
            "expected no app_label with default config, got:\n{result}"
        );
    }

    #[test]
    fn test_app_label_from_config_reaches_meta_class() {
        let table = users_table();
        let schema = vec![table.clone()];
        let mut config = DjangoConfig::default();
        config.app_label = Some("myapp".to_string());
        let exporter = DjangoExporterWithConfig::new(&config);
        let result = exporter.render_entity_with_schema(&table, &schema).unwrap();
        assert!(
            result.contains("        app_label = \"myapp\""),
            "expected app_label in Meta class, got:\n{result}"
        );
    }
}
