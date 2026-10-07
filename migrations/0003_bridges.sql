CREATE TABLE bridge_messages (
 destination_chain BIGINT NOT NULL, message_id BYTEA NOT NULL CHECK(octet_length(message_id)=32),
 source_data JSONB, destination_data JSONB, source_block BYTEA, destination_block BYTEA,
 source_canonical BOOLEAN NOT NULL DEFAULT false, destination_canonical BOOLEAN NOT NULL DEFAULT false,
 correlated BOOLEAN NOT NULL DEFAULT false, PRIMARY KEY(destination_chain,message_id)
);
CREATE INDEX bridge_source_block ON bridge_messages(source_block);
CREATE INDEX bridge_destination_block ON bridge_messages(destination_chain,destination_block);
