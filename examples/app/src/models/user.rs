use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user")]
pub struct Model {
    #[sea_orm(primary_key, default_value = "gen_random_uuid()")]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub email: String,
    pub password: String,
    pub name: String,
    pub profile_image: Option<String>,
    #[sea_orm(default_value = "now()")]
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(has_many)]
    pub article_users: HasMany<super::article_user::Entity>,
    #[sea_orm(has_many, via = "article_user")]
    pub articles: HasMany<super::article::Entity>,
    #[sea_orm(has_many)]
    pub medias: HasMany<super::media::Entity>,
    #[sea_orm(has_many)]
    pub user_media_roles: HasMany<super::user_media_role::Entity>,
    #[sea_orm(has_many, via = "user_media_role")]
    pub medias_1: HasMany<super::media::Entity>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_user_email on [email] unique=false
impl ActiveModelBehavior for ActiveModel {}
