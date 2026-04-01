use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "dual")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub username: String,
    #[sea_orm(has_many, relation_enum = "DualRel", via_rel = "Username")]
    pub username_dual_rels: HasMany<super::dual_rel::Entity>,
    #[sea_orm(
        has_many,
        relation_enum = "CheckerUsername",
        via_rel = "CheckerUsername"
    )]
    pub checker_username_dual_rels: HasMany<super::dual_rel::Entity>,
}

vespera::schema_type!(Schema from Model, name = "DualSchema");
impl ActiveModelBehavior for ActiveModel {}

pub struct UsernameToCheckerUsernameViaDualRel;
impl Linked for UsernameToCheckerUsernameViaDualRel {
    type FromEntity = Entity;
    type ToEntity = Entity;

    fn link(&self) -> Vec<RelationDef> {
        vec![
            super::dual_rel::Relation::Username.def().rev(),
            super::dual_rel::Relation::CheckerUsername.def(),
        ]
    }
}

pub struct CheckerUsernameToUsernameViaDualRel;
impl Linked for CheckerUsernameToUsernameViaDualRel {
    type FromEntity = Entity;
    type ToEntity = Entity;

    fn link(&self) -> Vec<RelationDef> {
        vec![
            super::dual_rel::Relation::CheckerUsername.def().rev(),
            super::dual_rel::Relation::Username.def(),
        ]
    }
}

impl Model {
    pub fn find_checker_usernames_via_dual_rel_from_username(&self) -> Select<Entity> {
        self.find_linked(UsernameToCheckerUsernameViaDualRel)
    }

    pub fn find_usernames_via_dual_rel_from_checker_username(&self) -> Select<Entity> {
        self.find_linked(CheckerUsernameToUsernameViaDualRel)
    }
}
