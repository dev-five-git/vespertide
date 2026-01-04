from __future__ import annotations

import enum
from datetime import datetime
from uuid import UUID

from sqlalchemy import DateTime, Enum, ForeignKey, Index, Uuid, text
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Role(str, enum.Enum):
    OWNER = "owner"
    EDITOR = "editor"
    REPORTER = "reporter"

class UserMediaRole(DeclarativeBase):
    """hello media role"""
    __tablename__ = "user_media_role"

    # hello
    user_id: Mapped[UUID] = mapped_column(Uuid, ForeignKey("user.id"), primary_key=True)
    media_id: Mapped[UUID] = mapped_column(Uuid, ForeignKey("media.id"), primary_key=True)
    role: Mapped[Role] = mapped_column(Enum(Role), nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False, server_default=text("now()"))

    __table_args__ = (
        Index(None, "user_id"),
        Index(None, "media_id"),
        Index(None, "role"),
    )
