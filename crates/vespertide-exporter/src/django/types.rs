use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::{DefaultValue, ReferenceAction};

use crate::utils::common::{is_jsonb_custom_type, string_literal, unquote};

#[derive(Default)]
pub(super) struct UsedImports {
    pub(super) needs_timezone: bool,
    pub(super) needs_uuid_default: bool,
}

pub(super) fn django_field_type(
    col_type: &ColumnType,
    is_pk: bool,
    auto_increment: bool,
) -> &'static str {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => {
                if is_pk && auto_increment {
                    "models.SmallAutoField"
                } else {
                    "models.SmallIntegerField"
                }
            }
            SimpleColumnType::Integer => {
                if is_pk && auto_increment {
                    "models.AutoField"
                } else {
                    "models.IntegerField"
                }
            }
            SimpleColumnType::BigInt => {
                if is_pk && auto_increment {
                    "models.BigAutoField"
                } else {
                    "models.BigIntegerField"
                }
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "models.FloatField",
            SimpleColumnType::Text | SimpleColumnType::Xml => "models.TextField",
            SimpleColumnType::Boolean => "models.BooleanField",
            SimpleColumnType::Date => "models.DateField",
            SimpleColumnType::Time => "models.TimeField",
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "models.DateTimeField",
            SimpleColumnType::Interval => "models.DurationField",
            SimpleColumnType::Bytea => "models.BinaryField",
            SimpleColumnType::Uuid => "models.UUIDField",
            SimpleColumnType::Json => "models.JSONField",
            SimpleColumnType::Inet | SimpleColumnType::Cidr => "models.GenericIPAddressField",
            SimpleColumnType::Macaddr => "models.CharField",
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                "models.CharField"
            }
            ComplexColumnType::Numeric { .. } => "models.DecimalField",
            // Postgres has no implicit `text -> jsonb` cast, so a `TextField` on a
            // JSONB column fails every write.
            ComplexColumnType::Custom { custom_type } if is_jsonb_custom_type(custom_type) => {
                "models.JSONField"
            }
            ComplexColumnType::Custom { .. } => "models.TextField",
            ComplexColumnType::Enum { values, .. } => match values {
                EnumValues::String(_) => "models.CharField",
                EnumValues::Integer(_) => "models.IntegerField",
            },
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "independent field-kwarg inputs, read once at a single call site"
)]
pub(super) fn build_field_kwargs(
    col_type: &ColumnType,
    is_pk: bool,
    is_unique: bool,
    nullable: bool,
    default: Option<&DefaultValue>,
    enum_class_name: Option<&str>,
    db_column: Option<&str>,
    used: &mut UsedImports,
) -> Vec<String> {
    let mut kwargs: Vec<String> = Vec::new();

    if let Some(db_col) = db_column {
        kwargs.push(format!("db_column={}", string_literal(db_col)));
    }

    // Size / precision kwargs
    match col_type {
        ColumnType::Complex(
            ComplexColumnType::Varchar { length } | ComplexColumnType::Char { length },
        ) => {
            kwargs.push(format!("max_length={length}"));
        }
        ColumnType::Simple(SimpleColumnType::Macaddr) => {
            kwargs.push("max_length=17".into());
        }
        ColumnType::Complex(ComplexColumnType::Numeric { precision, scale }) => {
            kwargs.push(format!("max_digits={precision}"));
            kwargs.push(format!("decimal_places={scale}"));
        }
        ColumnType::Complex(ComplexColumnType::Enum { values, .. }) => {
            if let Some(class) = enum_class_name {
                if let EnumValues::String(vals) = values {
                    let mut max_len = 1;
                    for v in vals {
                        if v.len() > max_len {
                            max_len = v.len();
                        }
                    }
                    kwargs.push(format!("max_length={max_len}"));
                }
                kwargs.push(format!("choices={class}.choices"));
            }
        }
        _ => {}
    }

    for (cond, kwarg) in [
        (is_pk, "primary_key=True"),
        (is_unique && !is_pk, "unique=True"),
    ] {
        if cond {
            kwargs.push(kwarg.into());
        }
    }
    if nullable && !is_pk {
        kwargs.push("null=True".into());
        kwargs.push("blank=True".into());
    }
    if let Some(dv) = default
        && let Some(expr) = build_default(col_type, &dv.to_sql(), used)
    {
        kwargs.push(format!("default={expr}"));
    }

    kwargs
}

pub(super) fn build_default(
    col_type: &ColumnType,
    sql: &str,
    used: &mut UsedImports,
) -> Option<String> {
    // A `JSONField` default has to be a callable (fields.E010), and the SQL
    // literal is the document's text, not its value. The database keeps its
    // own default.
    if django_field_type(col_type, false, false) == "models.JSONField" {
        return None;
    }

    if sql.contains('(') {
        let up = sql.to_uppercase();
        let is_timestamp_col = matches!(
            col_type,
            ColumnType::Simple(SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz)
        );
        if is_timestamp_col && (up.contains("NOW") || up.contains("CURRENT_TIMESTAMP")) {
            used.needs_timezone = true;
            return Some("timezone.now".into());
        }
        if matches!(col_type, ColumnType::Simple(SimpleColumnType::Uuid)) {
            used.needs_uuid_default = true;
            return Some("uuid.uuid4".into());
        }
        return None;
    }

    let up = sql.to_uppercase();
    if up == "TRUE" {
        return Some("True".into());
    }
    if up == "FALSE" {
        return Some("False".into());
    }

    if sql.len() >= 2 && sql.starts_with('\'') && sql.ends_with('\'') {
        // `unquote` keeps the doubled SQL escape (its other consumers re-emit
        // into SQL); a Python string wants the actual value.
        return Some(string_literal(&unquote(sql).replace("''", "'")));
    }

    // A bare numeric literal (e.g. "0", "-1.5") is valid Python as-is. Any
    // other bare, unquoted token is an unresolvable DB-level constant/
    // expression (e.g. a named SQL constant) — emitting it verbatim would
    // produce an undefined-name reference in the generated Python, so omit
    // the default entirely rather than guess.
    if sql.parse::<f64>().is_ok() {
        return Some(sql.into());
    }

    None
}

/// The `on_delete` a ForeignKey can carry; a key without an action restricts.
/// Django emulates the action itself and rejects one the field cannot carry
/// out: SET_DEFAULT without a default (fields.E321), SET_NULL on a field that
/// is not null (fields.E320). The table is unmanaged, so the database still
/// applies its own rule; DO_NOTHING leaves it to.
pub(super) fn on_delete_for(
    action: Option<&ReferenceAction>,
    has_default: bool,
    null: bool,
) -> &'static str {
    match action {
        Some(ReferenceAction::SetDefault) if !has_default => "models.DO_NOTHING",
        Some(ReferenceAction::SetNull) if !null => "models.DO_NOTHING",
        Some(action) => reference_action_str(action),
        None => "models.RESTRICT",
    }
}

fn reference_action_str(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "models.CASCADE",
        ReferenceAction::Restrict => "models.RESTRICT",
        ReferenceAction::SetNull => "models.SET_NULL",
        ReferenceAction::SetDefault => "models.SET_DEFAULT",
        ReferenceAction::NoAction => "models.DO_NOTHING",
    }
}
