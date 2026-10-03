from typing import Literal

from pydantic import BaseModel, ConfigDict


class Operation(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")

    id: str
    kind: Literal["ENTRY", "PICK_BAN", "AUCTION"]
    title: str
    status: str
    detail: str
    updatedAt: str
    href: str
    priority: Literal["urgent", "normal"]


class OperationsData(BaseModel):
    operations: list[Operation]
