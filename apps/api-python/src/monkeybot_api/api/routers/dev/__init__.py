"""Conditionally installed development-only API routes."""

from fastapi import APIRouter

from monkeybot_api.api.routers.dev import (
    groups,
    recruitments,
    user_lifecycle,
    user_links,
    users,
)

router = APIRouter()
router.include_router(groups.router)
router.include_router(recruitments.router)
router.include_router(users.router)
router.include_router(user_links.router)
router.include_router(user_lifecycle.router)
