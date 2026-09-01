// Shared verbatim with the SQLModel backend — both Python ORMs emit an
// identical Python `enum` class, so the single implementation lives in
// `crate::utils::python`.
pub(super) use crate::utils::python::render_enum;
