use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "article_user_role")]
pub enum ArticleUserRole {
    #[sea_orm(string_value = "lead")]
    Lead,
    #[sea_orm(string_value = "contributor")]
    Contributor,
}

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "article_user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub article_id: i64,
    #[sea_orm(primary_key)]
    pub user_id: Uuid,
    #[sea_orm(default_value = 1)]
    pub author_order: i32,
    #[sea_orm(default_value = ArticleUserRole::Contributor)]
    pub role: ArticleUserRole,
    #[sea_orm(default_value = "now()")]
    pub created_at: DateTimeWithTimeZone,
    #[sea_orm(belongs_to, from = "article_id", to = "id")]
    pub article: HasOne<super::article::Entity>,
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::user::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_article_user_user_id on [user_id] unique=false
impl ActiveModelBehavior for ActiveModel {}
