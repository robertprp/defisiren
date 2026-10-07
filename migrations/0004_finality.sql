CREATE TABLE finality_points (
 chain_id bigint NOT NULL, stage text NOT NULL CHECK(stage IN ('safe','finalized')),
 number bigint NOT NULL, hash bytea NOT NULL CHECK(octet_length(hash)=32),
 observed_at timestamptz NOT NULL, source text NOT NULL,
 PRIMARY KEY(chain_id,stage)
);
CREATE INDEX incidents_lifecycle ON incidents(chain_id,(data->>'finality'));
