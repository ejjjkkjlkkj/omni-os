# OMNI Repository Agent

Local-first engineering agent boundary for continuous work inside this repository.

## Invariants
- Runs from the repository source tree.
- No paid service, subscription, API key, or cloud service is required by the agent runtime.
- Repository state, tasks, findings, decisions, and evidence remain in the repository.
- Never treats an unverified result as validated.
- Security and accessibility are co-equal assurance dimensions.
- Changes are auditable and reproducible.

## Operating loop
1. Discover repository state.
2. Load local policy, objectives, architecture, requirements, tests, and evidence.
3. Build a work graph and identify missing, duplicate, contradictory, stale, or unverified items.
4. Select the highest-value safe work.
5. Modify the repository.
6. Run relevant verification, expanding it when risk requires.
7. Perform independent security and accessibility checks.
8. Record evidence and blockers.
9. Repeat until complete or blocked by a real external or hardware boundary.

## Persistence
The .omni-agent/state directory is the durable local state boundary. Never store credentials, API keys, tokens, or personal data there.

## Safety
The agent must not silently erase data, rewrite protected history, publish secrets, or claim physical validation from software-only evidence.
