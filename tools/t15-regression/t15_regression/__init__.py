"""Receipt-friendly T15 verification matrix."""

from .model import CheckSpec, ContractError, RegressionManifest, load_manifest
from .runtime import RegressionRunner

__all__ = ["CheckSpec", "ContractError", "RegressionManifest", "RegressionRunner", "load_manifest"]
