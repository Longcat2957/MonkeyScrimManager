"""Guild context, workflow reads, and operator administration endpoints."""

from fastapi import APIRouter

from monkeybot_api.api.routers.guilds import (
    audit_logs,
    context,
    full_data_transfer,
    leaderboard,
    legacy_data_transfer,
    members,
    operations,
    record_profile,
    record_search,
    seasons,
    settings,
    statistics,
)

router = APIRouter()
router.include_router(context.router)
router.include_router(full_data_transfer.router)
router.include_router(legacy_data_transfer.router)
router.include_router(settings.router)
router.include_router(seasons.router)
router.include_router(members.router)
router.include_router(audit_logs.router)
router.include_router(operations.router)
router.include_router(leaderboard.router)
router.include_router(statistics.router)
router.include_router(record_profile.router)
router.include_router(record_search.router)
