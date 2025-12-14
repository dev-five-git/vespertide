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
}

impl ActiveModelBehavior for ActiveModel {}
