CREATE TABLE chain_cursors (
 chain_id BIGINT PRIMARY KEY CHECK(chain_id > 0), block_number BIGINT NOT NULL,
 block_hash BYTEA NOT NULL CHECK(octet_length(block_hash)=32), engine JSONB NOT NULL,
 updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE canonical_blocks (
 chain_id BIGINT NOT NULL, number BIGINT NOT NULL, hash BYTEA NOT NULL CHECK(octet_length(hash)=32),
 parent_hash BYTEA NOT NULL, canonical BOOLEAN NOT NULL DEFAULT true, engine JSONB NOT NULL,
 observed_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY(chain_id,hash)
);
CREATE UNIQUE INDEX canonical_height ON canonical_blocks(chain_id,number) WHERE canonical;
CREATE TABLE observations (
 id BIGSERIAL PRIMARY KEY, chain_id BIGINT NOT NULL, hash BYTEA NOT NULL, source TEXT NOT NULL,
 stage TEXT NOT NULL, block_hash BYTEA NOT NULL, sender BYTEA NOT NULL, nonce NUMERIC(20,0) NOT NULL,
 data JSONB NOT NULL, canonical BOOLEAN NOT NULL, accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(chain_id,hash,source,stage,block_hash)
);
CREATE INDEX sender_nonce ON observations(chain_id,sender,nonce);
CREATE INDEX observation_block ON observations(chain_id,block_hash);
CREATE TABLE address_profiles (
 chain_id BIGINT NOT NULL, address BYTEA NOT NULL CHECK(octet_length(address)=20), data JSONB NOT NULL,
 PRIMARY KEY(chain_id,address)
);
CREATE TABLE funding_edges (
 id TEXT PRIMARY KEY, chain_id BIGINT NOT NULL, block_hash BYTEA, recipient BYTEA NOT NULL,
 data JSONB NOT NULL, canonical BOOLEAN NOT NULL
);
CREATE INDEX funding_recipient ON funding_edges(chain_id,recipient) WHERE canonical;
CREATE TABLE contracts (
 chain_id BIGINT NOT NULL, address BYTEA NOT NULL, state_hash BYTEA NOT NULL,
 code_hash BYTEA NOT NULL, analysis_version TEXT NOT NULL, data JSONB NOT NULL,
 PRIMARY KEY(chain_id,address,state_hash,analysis_version)
);
CREATE TABLE incidents (
 id UUID PRIMARY KEY, chain_id BIGINT NOT NULL, revision BIGINT NOT NULL, data JSONB NOT NULL,
 updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX incident_chain_time ON incidents(chain_id,updated_at DESC);
CREATE TABLE stream_heads (stream TEXT PRIMARY KEY, cursor BIGINT NOT NULL DEFAULT 0, floor BIGINT NOT NULL DEFAULT 0);
INSERT INTO stream_heads(stream) VALUES ('findings'),('reviewed');
CREATE TABLE finding_revisions (
 event_id UUID PRIMARY KEY, incident_id UUID NOT NULL REFERENCES incidents(id), revision BIGINT NOT NULL,
 chain_id BIGINT NOT NULL, block_hash BYTEA, data JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(incident_id,revision)
);
CREATE INDEX finding_block ON finding_revisions(chain_id,block_hash);
CREATE TABLE delivery_outbox (
 stream TEXT NOT NULL, cursor BIGINT NOT NULL, event_id UUID NOT NULL REFERENCES finding_revisions(event_id),
 data JSONB NOT NULL, telegram_sent BOOLEAN NOT NULL DEFAULT false, attempts INTEGER NOT NULL DEFAULT 0,
 next_attempt TIMESTAMPTZ NOT NULL DEFAULT now(), created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 PRIMARY KEY(stream,cursor), UNIQUE(stream,event_id)
);
CREATE INDEX pending_delivery ON delivery_outbox(next_attempt) WHERE NOT telegram_sent;
CREATE TABLE simulations (id UUID PRIMARY KEY, incident_id UUID, context_key TEXT NOT NULL UNIQUE, data JSONB NOT NULL);
CREATE TABLE response_policies (id UUID PRIMARY KEY, version INTEGER NOT NULL, data JSONB NOT NULL, enabled BOOLEAN NOT NULL DEFAULT false);
CREATE TABLE response_attempts (
 digest BYTEA PRIMARY KEY CHECK(octet_length(digest)=32), chain_id BIGINT NOT NULL, signer BYTEA NOT NULL,
 nonce NUMERIC(20,0) NOT NULL, policy_id UUID NOT NULL REFERENCES response_policies(id),
 status TEXT NOT NULL, data JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(chain_id,signer,nonce)
);
CREATE TABLE api_clients (id UUID PRIMARY KEY, key_hash BYTEA NOT NULL UNIQUE, name TEXT NOT NULL, enabled BOOLEAN NOT NULL DEFAULT true, expires_at TIMESTAMPTZ NOT NULL);
CREATE TABLE api_rate_limits (client_id UUID NOT NULL REFERENCES api_clients(id), minute BIGINT NOT NULL, count INTEGER NOT NULL, PRIMARY KEY(client_id,minute));
CREATE TABLE watchlist (chain_id BIGINT NOT NULL,address BYTEA NOT NULL,expires_at TIMESTAMPTZ NOT NULL,reason TEXT NOT NULL,PRIMARY KEY(chain_id,address));
CREATE TABLE capabilities (chain_id BIGINT PRIMARY KEY, data JSONB NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE TABLE evidence_objects (checksum TEXT PRIMARY KEY, location TEXT NOT NULL, bytes BIGINT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());

CREATE TABLE applied_journal (id BYTEA PRIMARY KEY CHECK(octet_length(id)=32), applied_at TIMESTAMPTZ NOT NULL DEFAULT now());
