from __future__ import annotations

import enum
from datetime import datetime
from uuid import UUID

from sqlalchemy import BigInteger, DateTime, Enum, ForeignKey, Index, Integer, Uuid, text
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Role(str, enum.Enum):
    LEAD = "lead"
    CONTRIBUTOR = "contributor"

class ArticleUser(DeclarativeBase):
    __tablename__ = "article_user"

    media_id: Mapped[UUID] = mapped_column(Uuid, primary_key=True)
    article_id: Mapped[int] = mapped_column(BigInteger, primary_key=True)
    user_id: Mapped[UUID] = mapped_column(Uuid, ForeignKey("user.id"), primary_key=True)
    author_order: Mapped[int] = mapped_column(Integer, nullable=False, server_default="1")
    role: Mapped[Role] = mapped_column(Enum(Role), nullable=False, server_default='contributor')
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False, server_default=text("now()"))

    __table_args__ = (
        Index(None, "user_id"),
    )
