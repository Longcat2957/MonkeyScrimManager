"""Auction recruitment, tournament, bracket, and match endpoints."""

from fastapi import APIRouter

from monkeybot_api.api.routers.auctions import (
    bracket,
    cancellation,
    finish,
    games,
    matches,
    recruitments,
    results,
    tournaments,
)

router = APIRouter()
router.include_router(recruitments.router)
router.include_router(tournaments.router)
router.include_router(bracket.router)
router.include_router(results.router)
router.include_router(matches.router)
router.include_router(games.router)
router.include_router(cancellation.router)
router.include_router(finish.router)
