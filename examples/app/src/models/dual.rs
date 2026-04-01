use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "dual")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub username: String,
    #[sea_orm(has_many)]
    pub dual_rels: HasMany<super::dual_rel::Entity>,
}

vespera::schema_type!(Schema from Model, name = "DualSchema");
impl ActiveModelBehavior for ActiveModel {}
