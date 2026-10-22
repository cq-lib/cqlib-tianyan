"""Evaluate a ZIP and require an explicit resource selection before v2 submit.

Uses an already authenticated platform supplied by the caller; does not load credentials.
"""
from pathlib import Path


def evaluate(platform, package: Path):
    schedule = platform.fusion_schedule(str(package), strategy="balanced")
    for candidate in schedule.candidates():
        print(candidate)
    return schedule


def submit_selected(platform, schedule, rank: int, name: str = "fusion-demo"):
    decision = schedule.select(rank)
    return platform.fusion_submit(name, decision)
