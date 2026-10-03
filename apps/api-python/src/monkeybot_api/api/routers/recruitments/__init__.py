"""Recruitment queries, commands, membership, and delivery endpoints."""

from fastapi import APIRouter

from monkeybot_api.api.routers.recruitments import creation, delivery, lifecycle, members, queries

router = APIRouter()
router.include_router(queries.router)
router.include_router(creation.router)
router.include_router(members.router)
router.include_router(lifecycle.router)
router.include_router(delivery.router)
