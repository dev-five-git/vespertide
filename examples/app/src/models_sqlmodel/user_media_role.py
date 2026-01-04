from __future__ import annotations

import enum
from datetime import datetime
from uuid import UUID

from sqlmodel import Field, SQLModel
from sqlalchemy import text


class Role(str, enum.Enum):
    OWNER = "owner"
    EDITOR = "editor"
    REPORTER = "reporter"

class UserMediaRole(SQLModel, table=True):
    """hello media role"""
    __tablename__ = "user_media_role"

    # hello
    user_id: UUID = Field(primary_key=True, foreign_key="user.id")
    media_id: UUID = Field(primary_key=True, foreign_key="media.id")
    role: Role = Field(index=True)
    created_at: datetime = Field(sa_column_kwargs={"server_default": text("now()")})
