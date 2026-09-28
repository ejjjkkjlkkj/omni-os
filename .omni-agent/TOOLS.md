# Agent tools contract

The agent is not only a data reader. It has a bounded local toolbelt.

## Required loop

inspect -> understand -> plan -> checkpoint -> modify -> test -> security audit -> accessibility audit -> regression -> evidence -> next task

## Tool classes

Read tools inspect the repository and history. Analysis tools build mappings and detect gaps. Verification tools execute local tests/builds. Write tools are bounded by policy and protected paths. Evidence tools record only non-secret provenance and results.

## Zero-cost boundary

The core toolbelt uses the Windows host, Python stdlib, Git and installed project toolchains. It does not require an API key, paid service, subscription or cloud model.

## Safety

The tool policy is deny-by-default for network shell commands, credentials, destructive operations and protected paths. Unknown test results remain UNKNOWN. A successful command does not by itself prove a requirement; evidence must be linked to the requirement and source.

## Security + accessibility

Security and accessibility audits are separate gates. A change cannot be considered complete when either gate is missing evidence.
