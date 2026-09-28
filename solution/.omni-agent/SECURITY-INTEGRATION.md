# Security knowledge boundary

The agent treats `ejjjkkjlkkj/omni-security` as a first-class engineering source,
not as an optional add-on.

## Required security inputs

- `SECURITY.md` and repository security policy.
- `requirements/` security and assurance requirements.
- `docs/` threat models, architecture and operational guidance.
- `tests/` security regression coverage.
- `.github/` CI/security automation and repository policy.
- `scripts/` validation and audit tooling.
- `data/` and `db/` only as documented, versioned knowledge sources.

## Agent rule

Every relevant change is checked against both the solution requirements and the
security-source requirements. Security and accessibility are independent
assurance dimensions: passing one never implies passing the other.

The agent records source commit SHAs and file hashes in `.omni-agent/state`.
A missing or unreadable required security source is a blocker, not a pass.

No credentials, tokens, API keys, or personal data may be copied into agent state.
