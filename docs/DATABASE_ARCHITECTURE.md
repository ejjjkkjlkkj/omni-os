# OMNI Database Architecture

## Purpose

OMNI's database is a normalized evidence-backed knowledge graph, not a single unstructured Dark Web dump.

The model preserves five distinct concepts:

1. Entity — the thing being represented.
2. Source — where information came from.
3. Evidence — the concrete observation supporting a claim.
4. Claim — a statement that can be verified or disputed.
5. Relationship — a typed link between entities.

The same model is used for cybersecurity and accessibility.

## Coverage model

Coverage is multidimensional:

- 23 stack/lifecycle layers from governance to physical/supply-chain depth.
- 21 publication/source planes already declared by the repository.
- 95 mapped domains in the current master map.
- Accessibility proof and screen-reader support are modeled independently from the security record.
- Optional sources remain represented and can be unverified, failed, blocked, or stale.

The database does not make a false completeness claim about inherently unindexed networks. Instead it records what is mapped, what has evidence, what was validated, and what remains a gap.

## Accessibility as a first-class security dimension

Accessibility has dedicated tables for requirements, capabilities, screen readers, speech engines, Braille, platform accessibility APIs, keyboard support, validation runs, test cases/results, compatibility, regressions, and layer-specific requirements.

This allows a security entity to have both security evidence and accessibility evidence without collapsing either into a note.

## Search / OSINT

Search engines, research tools, code repositories, and CTI feeds are separate concepts.

The current repository taxonomy contains 24 mapped search/reference entries. A repository URL alone does not make the repository a trusted data feed.

External code is reference-and-adapt-only.

## UEFI / firmware / root of trust

The infrastructure model explicitly represents platforms, firmware and versions, UEFI components, boot artifacts, measured boot, attestations, and root of trust.

Hashes and evidence references are retained so that later validation can distinguish an observed artifact from an unverified claim.

## Supply chain

The schema supports suppliers, components, packages, versions, direct/transitive dependencies, builds, artifacts, SBOM, attestations, signatures, and vulnerability links.

## Provenance

The intended provenance chain is:

source -> observation -> artifact/claim -> entity -> relationship

No importer should create an entity from an external record without retaining enough provenance to trace it back to its observation.

## Validation

Database validation must eventually cover migration integrity, foreign keys, uniqueness, taxonomy coverage, required/optional source state, evidence provenance, accessibility coverage, search mapping, UEFI/root-of-trust representation, supply-chain representation, and secret-safety invariants.

The migration is deliberately separated from ingestion so that schema correctness can be tested independently of external availability.

## PostgreSQL indexing

Structured JSON metadata uses jsonb where appropriate. PostgreSQL supports GIN indexes for JSONB containment/key queries and for tsvector full-text search. The schema therefore avoids indexing every field indiscriminately and keeps relational columns for stable query dimensions.
