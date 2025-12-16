use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub logo: Option<String>,
    pub owner_id: Uuid,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(belongs_to, from = "owner_id", to = "id")]
    pub user: HasOne<super::user::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_media_owner_id on [owner_id] unique=false
impl ActiveModelBehavior for ActiveModel {}
