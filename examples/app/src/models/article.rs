use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "article")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub title: String,
    pub content: String,
    pub summary: Option<String>,
    pub thumbnail: Option<String>,
    pub media_id: Uuid,
    pub status: String,
    pub published_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(belongs_to, from = "media_id", to = "id")]
    pub media: HasOne<super::media::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_article_media_id on [media_id] unique=false
// idx_article_status on [status] unique=false
// idx_article_published_at on [published_at] unique=false
impl ActiveModelBehavior for ActiveModel {}
