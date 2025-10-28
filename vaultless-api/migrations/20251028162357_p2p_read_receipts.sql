CREATE TABLE IF NOT EXISTS p2p_read_receipts (
    id UUID NOT NULL DEFAULT uuid_generate_v4(),
    message_id UUID NOT NULL,
    client_id UUID NOT NULL, -- The recipient who read the message
    read_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    CONSTRAINT p2p_read_receipts_pkey PRIMARY KEY (id),
    
    -- Ensures a recipient can only register one 'read' per message
    CONSTRAINT p2p_read_receipts_unique UNIQUE (message_id, client_id),
    
    -- Link to the message table
    CONSTRAINT p2p_read_receipts_message_fkey FOREIGN KEY (message_id)
        REFERENCES messages (id) ON DELETE CASCADE,
    
    -- Link to the client (recipient) table
    -- Assuming a 'clients' table exists
    CONSTRAINT p2p_read_receipts_client_fkey FOREIGN KEY (client_id)
        REFERENCES clients (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_p2p_receipts_message ON p2p_read_receipts(message_id);
CREATE INDEX IF NOT EXISTS idx_p2p_receipts_client ON p2p_read_receipts(client_id);

COMMENT ON TABLE p2p_read_receipts IS
'Tracks read confirmation for peer-to-peer (P2P) messages.';