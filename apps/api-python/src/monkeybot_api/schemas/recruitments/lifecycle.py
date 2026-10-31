from typing import Literal

from monkeybot_api.schemas.common import Revision
from monkeybot_api.schemas.recruitments.entries import MoveParticipant

Action = Literal["Close", "Cancel", "Restore", "Reopen", "Apply", "Reset"]


ENTRY_ACTIONS = {"Reopen", "Apply", "Reset"}


class EditRevision(Revision):
    operation: MoveParticipant
