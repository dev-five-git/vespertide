use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "post")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub title: String,
    pub content: String,
    pub created_at: DateTime,
    pub updated_at: Option<DateTime>,
    pub user_id: i32,
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::user::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// tuple on [updated_at, user_id] unique=false
// tuple2 on [updated_at, user_id] unique=false
impl ActiveModelBehavior for ActiveModel {}
