use vespertide_core::DefaultValue;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnKind, SimpleColumnType,
};

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
        ColumnType::Simple(ty) => match SimpleColumnKind::from(*ty) {
            SimpleColumnKind::SmallInt => {
                if is_pk && auto_increment {
                    "models.SmallAutoField"
                } else {
                    "models.SmallIntegerField"
                }
            }
            SimpleColumnKind::Integer => {
                if is_pk && auto_increment {
                    "models.AutoField"
                } else {
                    "models.IntegerField"
                }
            }
            SimpleColumnKind::BigInt => {
                if is_pk && auto_increment {
                    "models.BigAutoField"
                } else {
                    "models.BigIntegerField"
                }
            }
            SimpleColumnKind::Real | SimpleColumnKind::DoublePrecision => "models.FloatField",
            SimpleColumnKind::Text | SimpleColumnKind::Xml => "models.TextField",
            SimpleColumnKind::Boolean => "models.BooleanField",
            SimpleColumnKind::Date => "models.DateField",
            SimpleColumnKind::Time => "models.TimeField",
            SimpleColumnKind::Timestamp | SimpleColumnKind::Timestamptz => "models.DateTimeField",
            SimpleColumnKind::Interval => "models.DurationField",
            SimpleColumnKind::Bytea => "models.BinaryField",
            SimpleColumnKind::Uuid => "models.UUIDField",
            SimpleColumnKind::Json => "models.JSONField",
            SimpleColumnKind::Inet | SimpleColumnKind::Cidr => "models.GenericIPAddressField",
            SimpleColumnKind::Macaddr => "models.CharField",
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                "models.CharField"
            }
            ComplexColumnType::Numeric { .. } => "models.DecimalField",
            ComplexColumnType::Custom { .. } => "models.TextField",
            ComplexColumnType::Enum { values, .. } => match values {
                EnumValues::String(_) => "models.CharField",
                EnumValues::Integer(_) => "models.IntegerField",
            },
            // `#[non_exhaustive]` future-variant guard; unreachable today.
            #[cfg(not(tarpaulin_include))]
            _ => {
                unreachable!("ComplexColumnType is #[non_exhaustive]; all variants matched")
            }
        },
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "all params are independent field-kwarg inputs; a context struct would add noise without reducing coupling"
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
        kwargs.push(format!("db_column=\"{db_col}\""));
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

    if sql.starts_with('\'') && sql.ends_with('\'') && sql.len() >= 2 {
        let inner = &sql[1..sql.len() - 1];
        return Some(format!("\"{}\"", inner.replace('"', "\\\"")));
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

pub(super) fn reference_action_str(action: &vespertide_core::ReferenceAction) -> &'static str {
    use vespertide_core::ReferenceActionKind;
    match ReferenceActionKind::from(action) {
        ReferenceActionKind::Cascade => "models.CASCADE",
        ReferenceActionKind::Restrict => "models.RESTRICT",
        ReferenceActionKind::SetNull => "models.SET_NULL",
        ReferenceActionKind::SetDefault => "models.SET_DEFAULT",
        ReferenceActionKind::NoAction => "models.DO_NOTHING",
    }
}
