use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "media_role")]
pub enum MediaRole {
    #[sea_orm(string_value = "owner")]
    Owner,
    #[sea_orm(string_value = "editor")]
    Editor,
    #[sea_orm(string_value = "reporter")]
    Reporter,
}

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_media_role")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub user_id: Uuid,
    #[sea_orm(primary_key)]
    pub media_id: Uuid,
    pub role: MediaRole,
    pub created_at: DateTimeWithTimeZone,
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::user::Entity>,
    #[sea_orm(belongs_to, from = "media_id", to = "id")]
    pub media: HasOne<super::media::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_user_media_role_user_id on [user_id] unique=false
// idx_user_media_role_media_id on [media_id] unique=false
// idx_user_media_role_role on [role] unique=false
impl ActiveModelBehavior for ActiveModel {}
