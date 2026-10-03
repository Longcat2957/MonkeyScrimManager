"""Contracts for reusable development dummy-user groups."""

from typing import Annotated, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict, Field

from monkeybot_api.schemas.common import user_id

UserIdentifier = Annotated[str, AfterValidator(user_id)]


class GroupModel(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)


class CreateGroup(GroupModel):
    name: str
    description: str | None = None
    memberIds: list[UserIdentifier] = Field(default_factory=list)


class UpdateGroup(GroupModel):
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)
    name: str | None = None
    description: str | None = None


class ReplaceGroupMembers(GroupModel):
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)
    memberIds: list[UserIdentifier]


class GroupMember(GroupModel):
    userId: str
    displayName: str
    position: int = Field(ge=1, le=100)


class DummyGroup(GroupModel):
    id: str
    name: str
    description: str | None
    revision: int = Field(ge=0)
    memberCount: int = Field(ge=0, le=100)
    members: list[GroupMember]
    createdAt: str
    updatedAt: str


class StoredGroup(GroupModel):
    group: DummyGroup


class DeletedGroup(GroupModel):
    deleted: Literal[True]
