"""Commands for dummy-user profile changes and lifecycle transitions."""

from monkeybot_api.schemas.dev.users import Privacy, Strict


class UpdateUser(Strict):
    expectedRevision: int
    displayName: str | None = None
    avatarSeed: str | None = None
    notes: str | None = None
    privacy: Privacy | None = None


class ExpectedRevision(Strict):
    expectedRevision: int
