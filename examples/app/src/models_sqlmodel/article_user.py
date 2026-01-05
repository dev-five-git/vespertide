from __future__ import annotations

import enum
from datetime import datetime
from uuid import UUID

from sqlmodel import Field, SQLModel
from sqlalchemy import text


class Role(str, enum.Enum):
    LEAD = "lead"
    CONTRIBUTOR = "contributor"

class ArticleUser(SQLModel, table=True):
    __tablename__ = "article_user"

    media_id: UUID = Field(primary_key=True)
    article_id: int = Field(primary_key=True)
    user_id: UUID = Field(primary_key=True, foreign_key="user.id")
    author_order: int = Field(default=1)
    role: Role = Field(default="contributor")
    created_at: datetime = Field(sa_column_kwargs={"server_default": text("now()")})
