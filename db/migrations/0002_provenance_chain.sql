-- OMNI Security provenance-chain hardening
-- Migration: 0002_provenance_chain.sql
BEGIN;

CREATE TABLE evidence.observation_artifacts (
  observation_id uuid NOT NULL REFERENCES evidence.source_observations(id) ON DELETE CASCADE,
  artifact_id uuid NOT NULL REFERENCES evidence.artifacts(id) ON DELETE CASCADE,
  PRIMARY KEY (observation_id, artifact_id)
);

CREATE TABLE evidence.artifact_claims (
  artifact_id uuid NOT NULL REFERENCES evidence.artifacts(id) ON DELETE CASCADE,
  claim_id uuid NOT NULL REFERENCES evidence.claims(id) ON DELETE CASCADE,
  PRIMARY KEY (artifact_id, claim_id)
);

CREATE TABLE evidence.claim_entities (
  claim_id uuid NOT NULL REFERENCES evidence.claims(id) ON DELETE CASCADE,
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  role text NOT NULL,
  PRIMARY KEY (claim_id, entity_id, role)
);

CREATE INDEX observation_artifacts_artifact_idx
  ON evidence.observation_artifacts(artifact_id);
CREATE INDEX artifact_claims_claim_idx
  ON evidence.artifact_claims(claim_id);
CREATE INDEX claim_entities_entity_idx
  ON evidence.claim_entities(entity_id);

COMMIT;
