from __future__ import annotations

import enum
from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlmodel import Field, SQLModel
from sqlalchemy import text


class Status(str, enum.Enum):
    DRAFT = "draft"
    REVIEW = "review"
    PUBLISHED = "published"
    ARCHIVED = "archived"

class Article(SQLModel, table=True):
    __tablename__ = "article"

    media_id: UUID = Field(primary_key=True, foreign_key="media.id")
    id: int = Field(primary_key=True)
    title: str = Field(...)
    content: str = Field(...)
    summary: Optional[str] = Field(default=None)
    thumbnail: Optional[str] = Field(default=None)
    status: Status = Field(default="draft", index=True)
    published_at: Optional[datetime] = Field(default=None, index=True)
    created_at: datetime = Field(sa_column_kwargs={"server_default": text("now()")})
    updated_at: Optional[datetime] = Field(default=None)
