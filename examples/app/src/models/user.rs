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
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_user_email on [email] unique=false
impl ActiveModelBehavior for ActiveModel {}
