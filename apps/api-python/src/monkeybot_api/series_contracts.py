import re
from typing import Annotated, Any, Literal

from pydantic import AfterValidator, BaseModel, ConfigDict, Field

from monkeybot_api.errors import ApiError
from monkeybot_api.recruitment_lifecycle import Revision
from monkeybot_api.recruitment_members import user_id

UserId = Annotated[str, AfterValidator(user_id)]


class Strict(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")


class SetSide(Strict):
    type: Literal["SET_SIDE"]
    sidePreset: str


class ClearPick(Strict):
    type: Literal["CLEAR_PICK"]
    playerId: UserId


class SetPick(Strict):
    type: Literal["SET_PICK"]
    playerId: UserId
    championId: str


class ClearBan(Strict):
    type: Literal["CLEAR_BAN"]
    side: str
    slot: int = Field(ge=-32768, le=32767)


class SetBan(Strict):
    type: Literal["SET_BAN"]
    side: str
    slot: int = Field(ge=-32768, le=32767)
    championId: str


class Bulk(Strict):
    type: Literal["APPLY_BULK"]
    items: list[Annotated[SetPick | SetBan, Field(discriminator="type")]]


class PickBan(Revision):
    operation: Annotated[
        SetSide | SetPick | ClearPick | SetBan | ClearBan | Bulk, Field(discriminator="type")
    ]


class Bans(BaseModel):
    # Rust's snapshot DTO allows unknown fields.
    model_config = ConfigDict(strict=True, extra="ignore")
    BLUE: list[str | None]
    RED: list[str | None]


class Submit(Strict):
    setNumber: int = Field(ge=-32768, le=32767)
    expectedRevision: int = Field(ge=-(2**63), le=2**63 - 1)
    sidePreset: str
    winner: str
    picks: dict[UserId, str | None] | None = None
    bans: Bans | None = None
    patchVersion: str | None = None


class Early(Revision):
    outcome: Literal["TEAM_1", "TEAM_2", "DRAW"]


def invalid(action: str) -> ApiError:
    name, message = {
        "pickban": ("pickban", "픽밴 변경"),
        "submit": ("game", "게임 결과"),
        "last": ("game", "게임 결과"),
        "complete": ("early_complete", "시리즈 조기 종료"),
        "revert": ("lifecycle", "시리즈 상태 변경"),
        "delete": ("lifecycle", "시리즈 상태 변경"),
    }[action]
    return ApiError(
        422, f"invalid_series_{name}_request", f"{message} 요청 형식이 올바르지 않습니다."
    )


def champion(value: str) -> bool:
    return re.fullmatch(r"[a-zA-Z0-9]{1,64}", value) is not None


def validate(body: dict[str, Any], action: str) -> None:
    valid = body["expectedRevision"] >= 0
    if action == "pickban":
        op = body["operation"]
        items = op["items"] if op["type"] == "APPLY_BULK" else [op]
        valid &= 1 <= len(items) <= 20
        seen = set()
        for item in items:
            if "championId" in item:
                valid &= champion(item["championId"])
            if "side" in item:
                valid &= item["side"] in {"BLUE", "RED"} and 1 <= item["slot"] <= 5
            if "sidePreset" in item:
                valid &= item["sidePreset"] in {"DEFAULT", "SWAPPED"}
            key = (item.get("playerId"), item.get("side"), item.get("slot"))
            valid &= key not in seen
            seen.add(key)
    if action == "submit":
        valid &= (
            1 <= body["setNumber"] <= 5
            and body["sidePreset"] in {"DEFAULT", "SWAPPED"}
            and body["winner"] in {"BLUE", "RED"}
        )
        patch = body["patchVersion"]
        valid &= patch is None or 1 <= len(patch.encode()) <= 32
        if body["picks"] is not None:
            valid &= all(v is None or champion(v) for v in body["picks"].values())
        if body["bans"] is not None:
            valid &= all(
                len(v) == 5 and all(x is None or champion(x) for x in v)
                for v in body["bans"].values()
            )
    if not valid:
        raise invalid(action)
