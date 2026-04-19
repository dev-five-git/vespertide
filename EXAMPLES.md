# Complete Examples

## User Table with Enum Status

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "user",
  "columns": [
    { "name": "id", "type": "integer", "nullable": false, "primary_key": { "auto_increment": true } },
    { "name": "email", "type": "text", "nullable": false, "unique": true, "index": true },
    { "name": "name", "type": { "kind": "varchar", "length": 100 }, "nullable": false },
    {
      "name": "status",
      "type": { "kind": "enum", "name": "user_status", "values": ["pending", "active", "suspended", "deleted"] },
      "nullable": false,
      "default": "'pending'"
    },
    { "name": "metadata", "type": "json", "nullable": true },
    { "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" },
    { "name": "updated_at", "type": "timestamptz", "nullable": true }
  ]
}
```

## Order Table with Integer Enum and CHECK

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "order",
  "columns": [
    { "name": "id", "type": "uuid", "nullable": false, "primary_key": true, "default": "gen_random_uuid()" },
    {
      "name": "customer_id",
      "type": "integer",
      "nullable": false,
      "foreign_key": { "ref_table": "customer", "ref_columns": ["id"], "on_delete": "restrict" },
      "index": true
    },
    { "name": "total", "type": { "kind": "numeric", "precision": 10, "scale": 2 }, "nullable": false },
    {
      "name": "priority",
      "type": {
        "kind": "enum",
        "name": "order_priority",
        "values": [
          { "name": "low", "value": 0 },
          { "name": "normal", "value": 10 },
          { "name": "high", "value": 20 },
          { "name": "urgent", "value": 30 }
        ]
      },
      "nullable": false,
      "default": 10
    },
    {
      "name": "status",
      "type": { "kind": "enum", "name": "order_status", "values": ["pending", "confirmed", "shipped", "delivered", "cancelled"] },
      "nullable": false,
      "default": "'pending'"
    },
    { "name": "notes", "type": "text", "nullable": true },
    { "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }
  ],
  "constraints": [
    { "type": "check", "name": "check_total_positive", "expr": "total >= 0" }
  ]
}
```

## Many-to-Many Join Table

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "user_role",
  "columns": [
    {
      "name": "user_id",
      "type": "integer",
      "nullable": false,
      "primary_key": true,
      "foreign_key": { "ref_table": "user", "ref_columns": ["id"], "on_delete": "cascade" }
    },
    {
      "name": "role_id",
      "type": "integer",
      "nullable": false,
      "primary_key": true,
      "foreign_key": { "ref_table": "role", "ref_columns": ["id"], "on_delete": "cascade" },
      "index": true
    },
    { "name": "granted_at", "type": "timestamptz", "nullable": false, "default": "NOW()" },
    { "name": "granted_by", "type": "integer", "nullable": true, "foreign_key": "user.id" }
  ]
}
```
