"""Manifest-driven macOS T1 verification harness."""

from .model import ContractError, Manifest, load_manifest

__all__ = ["ContractError", "Manifest", "load_manifest"]
