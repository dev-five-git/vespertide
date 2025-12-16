use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "article_user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub article_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    pub author_order: i32,
    pub role: String,
    pub created_at: DateTimeWithTimeZone,
    #[sea_orm(belongs_to, from = "article_id", to = "id")]
    pub article: HasOne<super::article::Entity>,
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::user::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_article_user_article_id on [article_id] unique=false
// idx_article_user_user_id on [user_id] unique=false
impl ActiveModelBehavior for ActiveModel {}
