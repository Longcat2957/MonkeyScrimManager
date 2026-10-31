from pydantic import BaseModel, ConfigDict

LANES = ("TOP", "JUNGLE", "MID", "BOTTOM", "SUPPORT")


class PrivacyInput(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    publicProfile: bool
    showRiotIds: bool
    allowExternalRecordSearch: bool


class ChampionsInput(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    championIds: list[str]
