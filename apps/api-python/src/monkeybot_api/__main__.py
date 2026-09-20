import json
import logging
import math

import uvicorn
from pydantic import ValidationError

from monkeybot_api.app import create_app
from monkeybot_api.config import Settings


class JsonFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        data = {"level": record.levelname.lower(), "message": record.getMessage()}
        for key in (
            "request_id",
            "trace_id",
            "method",
            "route",
            "status",
            "latency_ms",
            "error_code",
            "failure_type",
            "sqlstate",
        ):
            if hasattr(record, key):
                data[key] = getattr(record, key)
        # Keep full unexpected exception diagnostics in server logs, never in HTTP responses.
        if record.exc_info:
            data["exception"] = self.formatException(record.exc_info)
        return json.dumps(data, ensure_ascii=False)


def main() -> None:
    handler = logging.StreamHandler()
    handler.setFormatter(JsonFormatter())
    logging.basicConfig(level=logging.INFO, handlers=[handler], force=True)
    try:
        settings = Settings()
    except ValidationError as error:
        # Never stringify ValidationError: validation context can include DSNs or OAuth secrets.
        fields = [".".join(map(str, item["loc"])) or "configuration" for item in error.errors()]
        logging.error("invalid_configuration", extra={"failure_type": ",".join(fields)})
        raise SystemExit(2) from None
    host, port = settings.bind_host_port()
    uvicorn.run(
        create_app(settings),
        host=host,
        port=port,
        workers=1,
        proxy_headers=False,
        access_log=False,
        log_config=None,
        timeout_graceful_shutdown=math.ceil(settings.shutdown_timeout_ms / 1000),
    )


if __name__ == "__main__":
    main()
