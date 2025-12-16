use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub email: String,
    pub password: String,
    pub name: String,
    pub profile_image: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: Option<DateTimeWithTimeZone>,
}


// Index definitions (SeaORM uses Statement builders externally)
// idx_user_email on [email] unique=false
impl ActiveModelBehavior for ActiveModel {}
