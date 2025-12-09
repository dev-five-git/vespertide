# vespertide
An open-source library for easily defining database schemas in JSON/YAML and automatically generating migration files

## Configuration
`vespertide-config` 크레이트를 사용해 프로젝트별 기본 경로와 네이밍 규칙을 관리할 수 있습니다.

```rust
use vespertide_config::{NameCase, VespertideConfig};

let config = VespertideConfig {
    models_dir: "models".into(),
    migrations_dir: "migrations".into(),
    table_naming_case: NameCase::Snake,
    column_naming_case: NameCase::Snake,
};
```