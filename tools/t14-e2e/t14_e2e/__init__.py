"""Manifest-driven native E2E ownership and evidence boundaries."""

from .model import ContractError, E2EManifest, FixtureResource, load_manifest
from .runtime import E2ERunner

__all__ = [
    "ContractError",
    "E2EManifest",
    "FixtureResource",
    "E2ERunner",
    "load_manifest",
]
