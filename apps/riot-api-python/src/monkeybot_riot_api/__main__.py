"""Run the standalone Riot service with one Uvicorn worker."""

import uvicorn

from .config import Settings
from .http import create_app


def main() -> None:
    settings = Settings()
    host, separator, port = settings.bind_addr.rpartition(":")
    if not separator or not host or not port.isdecimal() or not 0 <= int(port) <= 65535:
        raise ValueError("MONKEYBOT_RIOT_BIND_ADDR must be an IP address and port")
    uvicorn.run(create_app(settings), host=host.strip("[]"), port=int(port), workers=1)


if __name__ == "__main__":
    main()
