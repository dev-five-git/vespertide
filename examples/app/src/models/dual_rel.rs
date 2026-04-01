use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "dual_rel")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub username: String,
    #[sea_orm(primary_key)]
    pub checker_username: String,
    #[sea_orm(
        belongs_to,
        relation_enum = "Username",
        from = "username",
        to = "username"
    )]
    pub dual: HasOne<super::dual::Entity>,
    #[sea_orm(
        belongs_to,
        relation_enum = "CheckerUsername",
        from = "checker_username",
        to = "username"
    )]
    pub checker: HasOne<super::dual::Entity>,
}

vespera::schema_type!(Schema from Model, name = "DualRelSchema");
impl ActiveModelBehavior for ActiveModel {}
