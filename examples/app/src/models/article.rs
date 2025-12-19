use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "article_status")]
pub enum ArticleStatus {
    #[sea_orm(string_value = "draft")]
    Draft,
    #[sea_orm(string_value = "review")]
    Review,
    #[sea_orm(string_value = "published")]
    Published,
    #[sea_orm(string_value = "archived")]
    Archived,
}

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "article")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub media_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub summary: Option<String>,
    pub thumbnail: Option<String>,
    #[sea_orm(indexed, default_value = ArticleStatus::Draft)]
    pub status: ArticleStatus,
    #[sea_orm(indexed)]
    pub published_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(default_value = "now()")]
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(belongs_to, from = "media_id", to = "id")]
    pub media: HasOne<super::media::Entity>,
    #[sea_orm(has_many)]
    pub article_users: HasMany<super::article_user::Entity>,
    #[sea_orm(has_many, via = "article_user")]
    pub users: HasMany<super::user::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// (unnamed) on [status]
// (unnamed) on [published_at]
impl ActiveModelBehavior for ActiveModel {}
