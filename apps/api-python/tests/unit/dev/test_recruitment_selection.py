from monkeybot_api.services.dev.recruitments import build_preview


def test_preview_reports_selection_and_capacity_without_mutation():
    overview = {
        "recruitment": {
            "id": "01900000-0000-7000-8000-000000000001",
            "title": "Test",
            "status": "OPEN",
            "targetCount": 10,
            "joinedCount": 10,
            "remainingSlots": 0,
            "revision": 2,
        },
        "candidates": [
            {
                "userId": "dmy_01900000-0000-7000-8000-000000000002",
                "displayName": "Dummy",
                "mainLane": "MID",
                "profileState": "ACTIVE",
                "membershipStatus": "ACTIVE",
                "participationState": "AVAILABLE",
            }
        ],
    }
    user = overview["candidates"][0]["userId"]
    result = build_preview(overview, "ADD", [user, user])
    assert result["canApply"] is False
    assert result["currentRevision"] == 2
    assert result["selected"] == [overview["candidates"][0]]
    assert [issue["code"] for issue in result["issues"]] == [
        "invalid_dummy_participant_selection",
        "recruitment_capacity_exceeded",
    ]
    assert overview["recruitment"]["joinedCount"] == 10
