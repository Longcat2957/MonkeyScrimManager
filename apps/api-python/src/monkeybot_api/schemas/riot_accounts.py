from pydantic import BaseModel, ConfigDict


class RiotIdentity(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    gameName: str
    tagLine: str
    platform: str
