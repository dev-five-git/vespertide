from __future__ import annotations

from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlmodel import Field, SQLModel
from sqlalchemy import text


class User(SQLModel, table=True):
    __tablename__ = "user"

    id: UUID = Field(sa_column_kwargs={"server_default": text("gen_random_uuid()")}, primary_key=True)
    email: str = Field(unique=True, index=True)
    password: str = Field(...)
    name: str = Field(...)
    profile_image: Optional[str] = Field(default=None)
    created_at: datetime = Field(sa_column_kwargs={"server_default": text("now()")})
    updated_at: Optional[datetime] = Field(default=None)
