use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "triple_rel")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub username: String,
    #[sea_orm(primary_key)]
    pub checker_username: String,
    #[sea_orm(primary_key)]
    pub other_username: String,
    #[sea_orm(
        belongs_to,
        relation_enum = "Username",
        from = "username",
        to = "username"
    )]
    pub triple: HasOne<super::triple::Entity>,
    #[sea_orm(
        belongs_to,
        relation_enum = "CheckerUsername",
        from = "checker_username",
        to = "username"
    )]
    pub checker: HasOne<super::triple::Entity>,
    #[sea_orm(
        belongs_to,
        relation_enum = "OtherUsername",
        from = "other_username",
        to = "username"
    )]
    pub other: HasOne<super::triple::Entity>,
}

vespera::schema_type!(Schema from Model, name = "TripleRelSchema");
impl ActiveModelBehavior for ActiveModel {}
