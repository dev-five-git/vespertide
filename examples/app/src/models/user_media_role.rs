use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize, vespera::Schema)]
#[serde(rename_all = "camelCase")]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "user_media_role_role")]
pub enum Role {
    #[sea_orm(string_value = "owner")]
    Owner,
    #[sea_orm(string_value = "editor")]
    Editor,
    #[sea_orm(string_value = "reporter")]
    Reporter,
}

/// hello media role
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_media_role")]
pub struct Model {
    /// hello
    #[sea_orm(primary_key)]
    pub user_id: Uuid,
    #[sea_orm(primary_key)]
    pub media_id: Uuid,
    #[sea_orm(indexed)]
    pub role: Role,
    #[sea_orm(default_value = "now()")]
    pub created_at: DateTimeWithTimeZone,
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::user::Entity>,
    #[sea_orm(belongs_to, from = "media_id", to = "id")]
    pub media: HasOne<super::media::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// (unnamed) on [user_id]
// (unnamed) on [media_id]
// (unnamed) on [role]
vespera::schema_type!(Schema from Model, name = "UserMediaRoleSchema");
impl ActiveModelBehavior for ActiveModel {}
