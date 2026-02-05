use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media")]
pub struct Model {
    /// hello
    #[sea_orm(primary_key, default_value = "gen_random_uuid()")]
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub logo: Option<String>,
    #[sea_orm(indexed)]
    pub owner_id: Uuid,
    #[sea_orm(default_value = "now()")]
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(belongs_to, from = "owner_id", to = "id")]
    pub owner: HasOne<super::user::Entity>,
    #[sea_orm(has_many)]
    pub articles: HasMany<super::article::Entity>,
    #[sea_orm(has_many)]
    pub user_media_roles: HasMany<super::user_media_role::Entity>,
    #[sea_orm(has_many, via = "user_media_role")]
    pub users: HasMany<super::user::Entity>,
}

// Index definitions (SeaORM uses Statement builders externally)
// (unnamed) on [owner_id]
vespera::schema_type!(Schema from Model, name = "MediaSchema");
impl ActiveModelBehavior for ActiveModel {}
