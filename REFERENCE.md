# Reference

## Naming Conventions

| Item | Convention | Example |
|------|------------|---------|
| Tables | snake_case | `user_role` |
| Columns | snake_case | `created_at` |
| Indexes | `ix_{table}__{columns}` | `ix_user__email` |
| Unique | `uq_{table}__{columns}` | `uq_user__email` |
| Foreign Key | `fk_{table}__{columns}` | `fk_post__author_id` |
| Check | `check_{description}` | `check_positive_amount` |
| Enums | snake_case | `order_status` |

> **Note**: Auto-generated constraint names use double underscore `__` as separator.

---

## Quick Reference

```
SIMPLE TYPES                              COMPLEX TYPES
----------------------------------------  ----------------------------------------
integer, big_int, small_int   Numbers     { "kind": "varchar", "length": N }
real, double_precision        Floats      { "kind": "char", "length": N }
text                          Strings     { "kind": "numeric", "precision": P, "scale": S }
boolean                       Flags       { "kind": "enum", "name": "...", "values": [...] }
date, time, timestamp         Time        { "kind": "custom", "custom_type": "..." }
timestamptz, interval         Time+
uuid                          UUIDs       REFERENCE ACTIONS (snake_case!)
json                          JSON        ----------------------------------------
bytea                         Binary      cascade, restrict, set_null,
inet, cidr, macaddr           Network     set_default, no_action
xml                           XML

CONSTRAINT TYPES (inline preferred)       DATABASE BACKENDS
----------------------------------------  ----------------------------------------
primary_key, unique, index,               postgres (default), mysql, sqlite
foreign_key, check
```

---

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| Invalid enum in `on_delete` | PascalCase used | Use `"cascade"` not `"Cascade"` |
| Missing required property | `nullable` omitted | Add `"nullable": true/false` |
| Unknown column type | Typo in type name | Check column types table above |
| FK validation failed | Referenced table missing | Create referenced table first |
| NOT NULL without default | Adding column to existing table | Add `default` or use `fill_with` in revision |
