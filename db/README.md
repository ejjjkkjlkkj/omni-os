# OMNI Security PostgreSQL database

The database is a normalized knowledge store for cybersecurity, accessibility, provenance, CTI, search/OSINT, UEFI/firmware, root-of-trust, and supply-chain data.

## Design rules

- PostgreSQL is the primary relational store.
- Security and accessibility are co-equal dimensions.
- Taxonomy identifiers remain canonical and are imported from the repository's JSON/CSV sources.
- Search engines and research tools are references/tools, not automatically threat-intelligence feeds.
- Every imported fact can retain source observation and evidence provenance.
- Optional coverage remains visible as optional, unverified, stale, blocked, or missing; it is never silently treated as complete.
- Raw third-party material is not trusted merely because it was imported.
- Secrets and private-key material are not stored in the database.

## Migration

Apply db/migrations/0001_initial_schema.sql.

The migration creates the core, taxonomy, source, evidence, intel, accessibility, search, infrastructure, supply-chain, identity, audit, ingestion, and analytics schemas.

## Existing repository data

The database is downstream of the existing canonical data:

- data/taxonomy/master-coverage.json
- data/taxonomy/accessibility-full-stack.json
- data/taxonomy/accessibility-overlay-source-proof.json
- data/taxonomy/accessibility-stack-source-proof.json
- data/taxonomy/search-engine-coverage.json
- data/threat-intel/sources.json
- data/threat-intel/generated/security-knowledge.json
- data/threat-intel/generated/catalog.json
- data/threat-intel/generated/manifest.json
- requirements/SECURITY_ACCESSIBILITY_REQUIREMENTS.csv

The first migration is schema-only: it does not invent or embed generated threat-intelligence records. Importers must read the canonical repository data and preserve its provenance.

## Important distinction

This database does not claim that a finite database can enumerate every future or currently unreachable Dark Web service. Coverage is represented as a measured, source-backed map with explicit gaps and verification status.

PostgreSQL jsonb is used for genuinely variable records and metadata; GIN indexes are provided only where structured JSON or full-text search benefits from inverted indexing.
