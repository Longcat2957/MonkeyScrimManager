from typing import Any

from monkeybot_api.schemas.common import Revision


class Conversion(Revision):
    expectedStartAt: Any = None
