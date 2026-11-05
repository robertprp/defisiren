-- Reversible triage: keep canonical evidence, revisions, stream cursors and outbox intact.
CREATE TABLE IF NOT EXISTS incident_suppressions (
 incident_id UUID PRIMARY KEY REFERENCES incidents(id),
 reason TEXT NOT NULL,
 rule_version TEXT NOT NULL,
 archived_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Archive only zero-score envelope diagnostics for documented transaction types.
-- Recognition does not imply support for local execution or prove benign behavior.
INSERT INTO incident_suppressions(incident_id,reason,rule_version)
SELECT id,
 'Known chain transaction envelope incorrectly flagged by the non-Ethereum type > 2 rule. Raw evidence retained; not a security alert.',
 'envelope-recognition/2'
FROM incidents
WHERE data->>'finding_type'='unsupported_envelope'
 AND data->>'severity'='informational'
 AND data #>> '{components,0,points}'='0'
 AND data #>> '{evidence,0,kind}'='envelope'
 AND jsonb_array_length(data->'evidence')=1
 AND (
  (chain_id=56 AND data #>> '{evidence,0,data,type}' IN ('3','4')) OR
  (chain_id=8453 AND data #>> '{evidence,0,data,type}' IN ('4','126')) OR
  (chain_id=42161 AND data #>> '{evidence,0,data,type}' IN ('4','100','101','102','104','105','106'))
 )
ON CONFLICT(incident_id) DO NOTHING;

CREATE OR REPLACE VIEW incident_catalog AS
SELECT i.id, i.chain_id, i.updated_at,
 i.data || jsonb_build_object('triage',jsonb_build_object('archived',s.incident_id IS NOT NULL,'reason',s.reason)) AS data,
 CASE
  WHEN s.incident_id IS NOT NULL THEN 'archived'
  WHEN i.data->>'source' IN ('synthetic_demo','offline_replay') THEN 'demo'
  WHEN i.data->>'retracted'='true' THEN 'retracted'
  WHEN i.data->>'finding_type' IN ('unsupported_envelope','pending_alternative','preconfirmation_correction','pending_expired') THEN 'diagnostics'
  WHEN i.data->>'severity' IN ('medium','high','critical') THEN 'review'
  ELSE 'watch'
 END AS category
FROM incidents i LEFT JOIN incident_suppressions s ON s.incident_id=i.id;
