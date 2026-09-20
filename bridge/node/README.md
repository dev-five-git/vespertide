# vespertide

Declarative database schema management: define tables in JSON or YAML, diff
them against the migration history, and generate SQL for PostgreSQL, MySQL
and SQLite plus ORM code (SeaORM, SQLAlchemy, SQLModel, JPA, Prisma, Drizzle, GORM).

This package is the `vespertide` command-line tool as a native Node addon,
so no Rust toolchain is needed.

```bash
npm install -g vespertide
vespertide init

# or without installing
npx vespertide init
```

Every command, flag and exit code is the one the native binary has; see the
[project README](https://github.com/dev-five-git/vespertide#readme).
