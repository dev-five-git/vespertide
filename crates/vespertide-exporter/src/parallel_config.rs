//! Empirically tuned Rayon thresholds.
//!
//! Measured during the Wave 6 parallelization pass via the criterion
//! benchmarks in `crates/*/benches/`.

/// Schema-level `SeaORM` export already wins at 50 tables.
pub(crate) const SEAORM_EXPORT_PAR_TABLE_THRESHOLD: usize = 50;
pub(crate) const SEAORM_EXPORT_PAR_TABLE_MIN_LEN: usize = 8;

/// FK relation resolution remains profitable when rendering larger schemas.
pub(crate) const SEAORM_RELATION_PAR_FK_THRESHOLD: usize = 50;
pub(crate) const SEAORM_RELATION_PAR_FK_MIN_LEN: usize = 8;

/// Python ORM schema exports win at the existing 50-table threshold.
pub(crate) const SQLALCHEMY_EXPORT_PAR_TABLE_THRESHOLD: usize = 50;
pub(crate) const SQLMODEL_EXPORT_PAR_TABLE_THRESHOLD: usize = 50;
pub(crate) const JPA_EXPORT_PAR_TABLE_THRESHOLD: usize = 50;

pub(crate) const PYTHON_EXPORT_PAR_TABLE_MIN_LEN: usize = 8;
pub(crate) const JPA_EXPORT_PAR_TABLE_MIN_LEN: usize = 8;
