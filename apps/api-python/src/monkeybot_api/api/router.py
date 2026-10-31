"""Compose the HTTP API from domain routers."""

from fastapi import APIRouter

from monkeybot_api.api.routers import (
    auctions,
    auth,
    guilds,
    internal_guilds,
    notifications,
    realtime,
    recruitments,
    riot_accounts,
    riot_records,
    search_suggestions,
    series,
    service_admin,
    system,
    users,
)

router = APIRouter()
router.include_router(system.router)
router.include_router(internal_guilds.router)
router.include_router(service_admin.router)
router.include_router(auth.router)
router.include_router(users.router)
router.include_router(guilds.router)
router.include_router(recruitments.router)
router.include_router(riot_accounts.router)
router.include_router(riot_records.router)
router.include_router(realtime.router)
router.include_router(series.router)
router.include_router(notifications.router)
router.include_router(auctions.router)
router.include_router(search_suggestions.router)
