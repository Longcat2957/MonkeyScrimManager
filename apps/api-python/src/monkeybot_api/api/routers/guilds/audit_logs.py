from fastapi import APIRouter, Request
from starlette.responses import JSONResponse

from monkeybot_api.api.dependencies import DatabaseDep, SettingsDep
from monkeybot_api.api.sessions import resolve_session
from monkeybot_api.api.validation import guild_path
from monkeybot_api.core.errors import ApiError, success
from monkeybot_api.schemas.guilds.audit_logs import AuditLogQuery
from monkeybot_api.services.guilds.audit_logs import list_audit_logs

router = APIRouter()


@router.api_route("/api/guilds/{guild_id}/audit-logs", methods=["GET", "HEAD"])
async def audit_logs(
    request: Request, guild_id: str, settings: SettingsDep, database: DatabaseDep
) -> JSONResponse:
    guild_path(guild_id)
    pairs = list(request.query_params.multi_items())
    query = dict(pairs)
    if len(query) != len(pairs) or set(query) - {"cursor", "limit"}:
        raise ApiError.from_status(400)
    # Rust extracts the query shape before authentication, then validates its values.
    parameters = AuditLogQuery.model_validate(query)
    principal = await resolve_session(request, database, settings)
    cursor, limit = parameters.page_parameters()
    return success(
        request,
        await list_audit_logs(
            database, guild_id, principal["user_id"], cursor, limit, request.state.request_id
        ),
    )
