from __future__ import annotations

from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlmodel import Field, SQLModel
from sqlalchemy import text


class Media(SQLModel, table=True):
    __tablename__ = "media"

    # hello
    id: UUID = Field(sa_column_kwargs={"server_default": text("gen_random_uuid()")}, primary_key=True)
    name: str = Field(...)
    description: Optional[str] = Field(default=None)
    logo: Optional[str] = Field(default=None)
    owner_id: UUID = Field(foreign_key="user.id", index=True)
    created_at: datetime = Field(sa_column_kwargs={"server_default": text("now()")})
    updated_at: Optional[datetime] = Field(default=None)
