"""Autonoetic SDK typed exceptions."""


class AutonoeticSdkError(Exception):
    """Base class for SDK errors."""


class PolicyViolation(AutonoeticSdkError):
    """Raised when policy denies an operation."""


class RateLimitExceeded(AutonoeticSdkError):
    """Raised when gateway governors reject an operation."""


class ApprovalRequiredError(AutonoeticSdkError):
    """Raised when human approval is required before proceeding."""

    def __init__(self, secret_name: str, message: str) -> None:
        self.secret_name = secret_name
        super().__init__(message)
