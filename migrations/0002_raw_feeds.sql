CREATE TABLE raw_feed_messages (
 chain_id BIGINT NOT NULL, source TEXT NOT NULL, sequence NUMERIC(20,0),
 payload_hash BYTEA NOT NULL CHECK(octet_length(payload_hash)=32),
 data JSONB NOT NULL, observed_at TIMESTAMPTZ NOT NULL,
 PRIMARY KEY(chain_id,source,payload_hash)
);
CREATE INDEX raw_feed_sequence ON raw_feed_messages(chain_id,source,sequence);
