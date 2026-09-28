-- OMNI Security PostgreSQL foundational schema
-- Migration: 0001_initial_schema.sql
-- Security/accessibility are co-equal dimensions.
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE SCHEMA IF NOT EXISTS core;
CREATE SCHEMA IF NOT EXISTS taxonomy;
CREATE SCHEMA IF NOT EXISTS source;
CREATE SCHEMA IF NOT EXISTS evidence;
CREATE SCHEMA IF NOT EXISTS intel;
CREATE SCHEMA IF NOT EXISTS accessibility;
CREATE SCHEMA IF NOT EXISTS search;
CREATE SCHEMA IF NOT EXISTS infrastructure;
CREATE SCHEMA IF NOT EXISTS supply_chain;
CREATE SCHEMA IF NOT EXISTS identity;
CREATE SCHEMA IF NOT EXISTS audit;
CREATE SCHEMA IF NOT EXISTS ingestion;
CREATE SCHEMA IF NOT EXISTS analytics;

CREATE TABLE taxonomy.statuses (
  id text PRIMARY KEY,
  description text NOT NULL
);

CREATE TABLE taxonomy.entity_types (
  id text PRIMARY KEY,
  description text NOT NULL
);

CREATE TABLE taxonomy.relationship_types (
  id text PRIMARY KEY,
  description text NOT NULL
);

CREATE TABLE taxonomy.domains (
  id text PRIMARY KEY,
  group_name text NOT NULL,
  description text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE taxonomy.stack_layers (
  id text PRIMARY KEY,
  layer_number integer NOT NULL UNIQUE,
  name text NOT NULL UNIQUE,
  description text
);

CREATE TABLE taxonomy.publication_layers (
  id text PRIMARY KEY,
  name text NOT NULL UNIQUE,
  description text
);

CREATE TABLE core.entities (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  canonical_id text NOT NULL UNIQUE,
  entity_type text NOT NULL REFERENCES taxonomy.entity_types(id),
  name text,
  description text,
  status text NOT NULL REFERENCES taxonomy.statuses(id),
  confidence numeric(5,4) CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  first_seen_at timestamptz,
  last_seen_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX entities_type_idx ON core.entities(entity_type);
CREATE INDEX entities_status_idx ON core.entities(status);
CREATE INDEX entities_metadata_gin_idx ON core.entities USING gin(metadata);

CREATE TABLE source.sources (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  canonical_id text NOT NULL UNIQUE,
  name text NOT NULL,
  kind text NOT NULL,
  domain text,
  url text,
  publication_layer_id text REFERENCES taxonomy.publication_layers(id),
  required boolean NOT NULL DEFAULT false,
  mode text NOT NULL CHECK (mode IN ('automatic','reference','import-only')),
  status text NOT NULL DEFAULT 'configured',
  trust_level text,
  first_seen_at timestamptz,
  last_checked_at timestamptz,
  last_success_at timestamptz,
  etag text,
  last_modified timestamptz,
  content_hash text,
  license text,
  terms text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX sources_layer_idx ON source.sources(publication_layer_id);
CREATE INDEX sources_status_idx ON source.sources(status);
CREATE INDEX sources_metadata_gin_idx ON source.sources USING gin(metadata);

CREATE TABLE core.entity_domains (
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  domain_id text NOT NULL REFERENCES taxonomy.domains(id),
  PRIMARY KEY(entity_id, domain_id)
);

CREATE TABLE core.entity_stack_layers (
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  stack_layer_id text NOT NULL REFERENCES taxonomy.stack_layers(id),
  PRIMARY KEY(entity_id, stack_layer_id)
);

CREATE TABLE core.entity_publication_layers (
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  publication_layer_id text NOT NULL REFERENCES taxonomy.publication_layers(id),
  PRIMARY KEY(entity_id, publication_layer_id)
);

CREATE TABLE core.entity_relationships (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  source_entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  target_entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  relationship_type text NOT NULL REFERENCES taxonomy.relationship_types(id),
  confidence numeric(5,4) CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  first_seen_at timestamptz,
  last_seen_at timestamptz,
  source_evidence_id uuid,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  CHECK (source_entity_id <> target_entity_id)
);

CREATE INDEX entity_rel_source_idx ON core.entity_relationships(source_entity_id);
CREATE INDEX entity_rel_target_idx ON core.entity_relationships(target_entity_id);
CREATE INDEX entity_rel_type_idx ON core.entity_relationships(relationship_type);

CREATE TABLE evidence.evidence (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  evidence_type text NOT NULL,
  validation_status text NOT NULL DEFAULT 'unverified',
  confidence numeric(5,4) CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  observed_at timestamptz NOT NULL,
  collected_at timestamptz,
  collector text,
  method text,
  mime_type text,
  byte_size bigint CHECK (byte_size IS NULL OR byte_size >= 0),
  content_hash text,
  locator text,
  content_ref text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE UNIQUE INDEX evidence_hash_idx ON evidence.evidence(content_hash)
WHERE content_hash IS NOT NULL;

CREATE TABLE evidence.source_observations (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  source_id uuid NOT NULL REFERENCES source.sources(id),
  evidence_id uuid NOT NULL UNIQUE REFERENCES evidence.evidence(id) ON DELETE CASCADE,
  external_id text,
  observed_at timestamptz NOT NULL,
  source_version text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(source_id, external_id, observed_at)
);

CREATE TABLE evidence.artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  evidence_id uuid NOT NULL REFERENCES evidence.evidence(id) ON DELETE CASCADE,
  artifact_type text NOT NULL,
  name text,
  media_type text,
  content_hash text,
  size_bytes bigint CHECK (size_bytes IS NULL OR size_bytes >= 0),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE evidence.claims (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  claim_type text NOT NULL,
  statement text NOT NULL,
  status text NOT NULL DEFAULT 'unverified',
  confidence numeric(5,4) CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  first_observed_at timestamptz,
  last_verified_at timestamptz,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX claims_statement_fts_idx ON evidence.claims
USING gin (to_tsvector('simple', statement));

CREATE TABLE evidence.claim_sources (
  claim_id uuid NOT NULL REFERENCES evidence.claims(id) ON DELETE CASCADE,
  evidence_id uuid NOT NULL REFERENCES evidence.evidence(id) ON DELETE CASCADE,
  PRIMARY KEY(claim_id, evidence_id)
);

CREATE TABLE evidence.entity_evidence (
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  evidence_id uuid NOT NULL REFERENCES evidence.evidence(id) ON DELETE CASCADE,
  role text NOT NULL,
  PRIMARY KEY(entity_id, evidence_id, role)
);

ALTER TABLE core.entity_relationships
  ADD CONSTRAINT entity_relationships_evidence_fk
  FOREIGN KEY(source_evidence_id) REFERENCES evidence.evidence(id);

CREATE TABLE intel.objects (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  external_id text,
  external_source text,
  object_type text NOT NULL,
  object_version text,
  created_at_external timestamptz,
  modified_at_external timestamptz,
  raw jsonb NOT NULL DEFAULT '{}'::jsonb,
  normalized jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(external_source, external_id)
);

CREATE INDEX intel_objects_raw_gin_idx ON intel.objects USING gin(raw);
CREATE INDEX intel_objects_normalized_gin_idx ON intel.objects USING gin(normalized);

CREATE TABLE intel.indicators (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid REFERENCES core.entities(id) ON DELETE SET NULL,
  indicator_type text NOT NULL,
  value text NOT NULL,
  normalized_value text NOT NULL,
  confidence numeric(5,4) CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  first_seen timestamptz,
  last_seen timestamptz,
  valid_from timestamptz,
  valid_until timestamptz,
  source_evidence_id uuid REFERENCES evidence.evidence(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(indicator_type, normalized_value)
);

CREATE INDEX indicators_normalized_idx ON intel.indicators(normalized_value);
CREATE INDEX indicators_metadata_gin_idx ON intel.indicators USING gin(metadata);

CREATE TABLE infrastructure.platforms (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  vendor text,
  device text,
  board text,
  architecture text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE infrastructure.firmware (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  vendor text,
  family text,
  firmware_type text,
  secure_boot_supported boolean,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE infrastructure.firmware_versions (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  firmware_id uuid NOT NULL REFERENCES infrastructure.firmware(id) ON DELETE CASCADE,
  version text NOT NULL,
  release_date date,
  content_hash text,
  signer text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(firmware_id, version)
);

CREATE TABLE infrastructure.uefi_components (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  firmware_version_id uuid REFERENCES infrastructure.firmware_versions(id) ON DELETE CASCADE,
  component_type text NOT NULL,
  name text,
  path text,
  content_hash text,
  signer text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE infrastructure.boot_artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid REFERENCES core.entities(id) ON DELETE SET NULL,
  artifact_type text NOT NULL,
  name text,
  path text,
  content_hash text,
  signer text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE infrastructure.boot_measurements (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  platform_id uuid REFERENCES infrastructure.platforms(id) ON DELETE SET NULL,
  component_id uuid REFERENCES infrastructure.uefi_components(id) ON DELETE SET NULL,
  pcr_bank text,
  pcr_index integer,
  digest text,
  algorithm text,
  measured_at timestamptz,
  evidence_id uuid REFERENCES evidence.evidence(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE infrastructure.attestations (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid REFERENCES core.entities(id) ON DELETE SET NULL,
  mechanism text NOT NULL,
  status text NOT NULL,
  evidence_id uuid REFERENCES evidence.evidence(id),
  issued_at timestamptz,
  expires_at timestamptz,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE infrastructure.root_of_trust (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  mechanism text NOT NULL,
  component text,
  lifecycle_state text,
  attestation_supported boolean,
  evidence_id uuid REFERENCES evidence.evidence(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.suppliers (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  name text,
  country text,
  risk_status text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.components (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  supplier_id uuid REFERENCES supply_chain.suppliers(id),
  component_type text NOT NULL,
  purl text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.packages (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  ecosystem text,
  name text NOT NULL,
  namespace text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.versions (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  package_id uuid REFERENCES supply_chain.packages(id) ON DELETE CASCADE,
  component_id uuid REFERENCES supply_chain.components(id) ON DELETE CASCADE,
  version text NOT NULL,
  purl text,
  checksum text,
  released_at timestamptz,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.dependencies (
  parent_version_id uuid NOT NULL REFERENCES supply_chain.versions(id) ON DELETE CASCADE,
  child_version_id uuid NOT NULL REFERENCES supply_chain.versions(id) ON DELETE CASCADE,
  dependency_type text NOT NULL DEFAULT 'runtime',
  direct boolean NOT NULL DEFAULT true,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  PRIMARY KEY(parent_version_id, child_version_id, dependency_type),
  CHECK(parent_version_id <> child_version_id)
);

CREATE TABLE supply_chain.builds (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid REFERENCES core.entities(id) ON DELETE SET NULL,
  builder text,
  build_id text,
  started_at timestamptz,
  finished_at timestamptz,
  provenance jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  build_id uuid REFERENCES supply_chain.builds(id) ON DELETE SET NULL,
  name text NOT NULL,
  digest text,
  media_type text,
  provenance jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.sbom (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  artifact_id uuid REFERENCES supply_chain.artifacts(id) ON DELETE CASCADE,
  format text NOT NULL CHECK (format IN ('spdx','cyclonedx','other')),
  version text,
  content_hash text NOT NULL,
  document jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE supply_chain.attestations (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  artifact_id uuid REFERENCES supply_chain.artifacts(id) ON DELETE CASCADE,
  framework text NOT NULL,
  predicate_type text,
  statement jsonb NOT NULL DEFAULT '{}'::jsonb,
  content_hash text,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE supply_chain.signatures (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  artifact_id uuid REFERENCES supply_chain.artifacts(id) ON DELETE CASCADE,
  mechanism text NOT NULL,
  signer text,
  signature_ref text,
  verified boolean NOT NULL DEFAULT false,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE supply_chain.vulnerabilities (
  version_id uuid NOT NULL REFERENCES supply_chain.versions(id) ON DELETE CASCADE,
  vulnerability_entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  source_evidence_id uuid REFERENCES evidence.evidence(id),
  PRIMARY KEY(version_id, vulnerability_entity_id)
);

CREATE TABLE accessibility.requirements (
  id text PRIMARY KEY,
  name text NOT NULL,
  description text NOT NULL,
  security_property text,
  accessibility_property text,
  required_evidence text,
  status text NOT NULL
);

CREATE TABLE accessibility.capabilities (
  id text PRIMARY KEY,
  name text NOT NULL,
  category text NOT NULL,
  description text
);

CREATE TABLE accessibility.screen_readers (
  id text PRIMARY KEY,
  name text NOT NULL,
  platform text,
  source_entity_id uuid REFERENCES core.entities(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.speech_engines (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name text NOT NULL,
  platform text,
  version text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.braille (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name text NOT NULL,
  protocol text,
  platform text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.platform_apis (
  id text PRIMARY KEY,
  name text NOT NULL,
  platform text,
  source_entity_id uuid REFERENCES core.entities(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.keyboard_support (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  capability_id text NOT NULL REFERENCES accessibility.capabilities(id),
  status text NOT NULL,
  evidence_id uuid REFERENCES evidence.evidence(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(entity_id, capability_id)
);

CREATE TABLE accessibility.validation_runs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid REFERENCES core.entities(id) ON DELETE SET NULL,
  screen_reader_id text REFERENCES accessibility.screen_readers(id),
  api_id text REFERENCES accessibility.platform_apis(id),
  run_type text NOT NULL,
  status text NOT NULL CHECK (status IN ('unknown','not_tested','partial','validated','regression','blocked')),
  started_at timestamptz,
  finished_at timestamptz,
  evidence_id uuid REFERENCES evidence.evidence(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.test_cases (
  id text PRIMARY KEY,
  name text NOT NULL,
  capability_id text REFERENCES accessibility.capabilities(id),
  description text NOT NULL
);

CREATE TABLE accessibility.test_results (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  validation_run_id uuid NOT NULL REFERENCES accessibility.validation_runs(id) ON DELETE CASCADE,
  test_case_id text NOT NULL REFERENCES accessibility.test_cases(id),
  status text NOT NULL CHECK (status IN ('pass','fail','blocked','not_tested')),
  evidence_id uuid REFERENCES evidence.evidence(id),
  details jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(validation_run_id, test_case_id)
);

CREATE TABLE accessibility.evidence (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  evidence_id uuid NOT NULL UNIQUE REFERENCES evidence.evidence(id) ON DELETE CASCADE,
  accessibility_status text NOT NULL,
  screen_reader text,
  braille_status text,
  keyboard_status text,
  speech_status text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.regressions (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  capability_id text REFERENCES accessibility.capabilities(id),
  detected_at timestamptz NOT NULL DEFAULT now(),
  previous_status text,
  current_status text NOT NULL,
  evidence_id uuid REFERENCES evidence.evidence(id),
  resolved_at timestamptz,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE accessibility.compatibility (
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  screen_reader_id text REFERENCES accessibility.screen_readers(id),
  api_id text REFERENCES accessibility.platform_apis(id),
  status text NOT NULL,
  last_validated_at timestamptz,
  evidence_id uuid REFERENCES evidence.evidence(id),
  PRIMARY KEY(entity_id, screen_reader_id, api_id)
);

CREATE TABLE accessibility.entity_layer_requirements (
  entity_id uuid NOT NULL REFERENCES core.entities(id) ON DELETE CASCADE,
  stack_layer_id text NOT NULL REFERENCES taxonomy.stack_layers(id),
  requirement_id text NOT NULL REFERENCES accessibility.requirements(id),
  status text NOT NULL,
  evidence_id uuid REFERENCES evidence.evidence(id),
  confidence numeric(5,4) CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
  last_validated_at timestamptz,
  PRIMARY KEY(entity_id, stack_layer_id, requirement_id)
);

CREATE TABLE search.engines (
  id text PRIMARY KEY,
  name text NOT NULL,
  category text NOT NULL,
  code_reference_status text NOT NULL,
  repository_url text,
  publication_layer_id text REFERENCES taxonomy.publication_layers(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE search.engine_layers (
  engine_id text NOT NULL REFERENCES search.engines(id) ON DELETE CASCADE,
  publication_layer_id text NOT NULL REFERENCES taxonomy.publication_layers(id),
  PRIMARY KEY(engine_id, publication_layer_id)
);

CREATE TABLE search.engine_capabilities (
  engine_id text NOT NULL REFERENCES search.engines(id) ON DELETE CASCADE,
  capability_id text NOT NULL REFERENCES accessibility.capabilities(id),
  PRIMARY KEY(engine_id, capability_id)
);

CREATE TABLE search.code_sources (
  id text PRIMARY KEY,
  repository text NOT NULL,
  license text,
  role text NOT NULL,
  retrieval_policy text NOT NULL CHECK (retrieval_policy = 'reference-and-adapt-only'),
  security_review_status text NOT NULL DEFAULT 'unverified',
  accessibility_review_status text NOT NULL DEFAULT 'unverified',
  last_verified_at timestamptz,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE search.runtime_validations (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  engine_id text NOT NULL REFERENCES search.engines(id) ON DELETE CASCADE,
  validation_run_id uuid NOT NULL REFERENCES accessibility.validation_runs(id) ON DELETE CASCADE,
  keyboard boolean,
  screen_reader boolean,
  focus_navigation boolean,
  heading_navigation boolean,
  form_navigation boolean,
  result_navigation boolean,
  dynamic_content boolean,
  aria_validation boolean,
  speech_output boolean,
  braille boolean,
  status text NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE search.query_runs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  engine_id text NOT NULL REFERENCES search.engines(id),
  query_hash text NOT NULL,
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  status text NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE search.results (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  query_run_id uuid NOT NULL REFERENCES search.query_runs(id) ON DELETE CASCADE,
  rank integer,
  title text,
  url text,
  snippet text,
  content_hash text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE search.result_sources (
  result_id uuid NOT NULL REFERENCES search.results(id) ON DELETE CASCADE,
  source_id uuid NOT NULL REFERENCES source.sources(id),
  PRIMARY KEY(result_id, source_id)
);

CREATE TABLE identity.identities (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_id uuid NOT NULL UNIQUE REFERENCES core.entities(id) ON DELETE CASCADE,
  identity_type text NOT NULL,
  assurance_level text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE identity.credentials (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  identity_id uuid NOT NULL REFERENCES identity.identities(id) ON DELETE CASCADE,
  credential_type text NOT NULL,
  public_material text,
  secret_material_ref text,
  status text NOT NULL,
  issued_at timestamptz,
  expires_at timestamptz,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  CHECK (secret_material_ref IS NULL OR secret_material_ref !~ '(BEGIN (RSA|EC|OPENSSH|PRIVATE) KEY|api[_-]?key|password|token)')
);

CREATE TABLE audit.events (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  event_type text NOT NULL,
  actor text,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  result text NOT NULL,
  source_id uuid REFERENCES source.sources(id),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE audit.changes (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  event_id uuid REFERENCES audit.events(id) ON DELETE SET NULL,
  table_name text NOT NULL,
  row_id uuid,
  operation text NOT NULL CHECK (operation IN ('insert','update','delete')),
  old_hash text,
  new_hash text,
  error text,
  changed_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE audit.validation_runs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  validator text NOT NULL,
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  status text NOT NULL,
  summary jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE audit.validation_results (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  validation_run_id uuid NOT NULL REFERENCES audit.validation_runs(id) ON DELETE CASCADE,
  check_id text NOT NULL,
  status text NOT NULL,
  details jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE ingestion.jobs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_type text NOT NULL,
  status text NOT NULL,
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  error text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE ingestion.job_sources (
  job_id uuid NOT NULL REFERENCES ingestion.jobs(id) ON DELETE CASCADE,
  source_id uuid NOT NULL REFERENCES source.sources(id),
  status text NOT NULL,
  records_seen bigint NOT NULL DEFAULT 0,
  records_imported bigint NOT NULL DEFAULT 0,
  records_rejected bigint NOT NULL DEFAULT 0,
  PRIMARY KEY(job_id, source_id)
);

CREATE TABLE ingestion.raw_artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid REFERENCES ingestion.jobs(id) ON DELETE SET NULL,
  source_id uuid REFERENCES source.sources(id),
  content_hash text NOT NULL,
  media_type text,
  size_bytes bigint CHECK(size_bytes IS NULL OR size_bytes >= 0),
  locator text,
  content_ref text,
  received_at timestamptz NOT NULL DEFAULT now(),
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  UNIQUE(source_id, content_hash)
);

CREATE TABLE ingestion.normalization_runs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid REFERENCES ingestion.jobs(id) ON DELETE SET NULL,
  raw_artifact_id uuid REFERENCES ingestion.raw_artifacts(id) ON DELETE SET NULL,
  status text NOT NULL,
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  records_created bigint NOT NULL DEFAULT 0,
  records_updated bigint NOT NULL DEFAULT 0,
  records_rejected bigint NOT NULL DEFAULT 0,
  error text
);

CREATE TABLE ingestion.errors (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid REFERENCES ingestion.jobs(id) ON DELETE CASCADE,
  source_id uuid REFERENCES source.sources(id),
  external_id text,
  error_code text NOT NULL,
  message text NOT NULL,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  retryable boolean NOT NULL DEFAULT false,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE ingestion.checkpoints (
  source_id uuid PRIMARY KEY REFERENCES source.sources(id) ON DELETE CASCADE,
  cursor text,
  source_version text,
  content_hash text,
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE VIEW analytics.coverage_matrix AS
SELECT
  d.id AS domain,
  sl.id AS stack_layer,
  pl.id AS publication_layer,
  s.canonical_id AS source,
  s.required,
  NOT s.required AS optional,
  COALESCE((SELECT count(*) FROM evidence.source_observations so WHERE so.source_id=s.id),0) AS observations,
  s.status,
  s.last_success_at AS last_validated_at
FROM taxonomy.domains d
CROSS JOIN taxonomy.stack_layers sl
CROSS JOIN taxonomy.publication_layers pl
LEFT JOIN source.sources s
  ON s.publication_layer_id = pl.id
  AND s.domain = d.id;

CREATE VIEW analytics.coverage_gaps AS
SELECT *
FROM analytics.coverage_matrix
WHERE source IS NULL
   OR status IN ('unverified','failed','blocked','stale');

COMMIT;
