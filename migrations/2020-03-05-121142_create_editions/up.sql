CREATE TABLE edition (
    id uuid default gen_random_uuid(),
    source_room_id uuid NOT NULL,
    created_by agent_id NOT NULL,
    created_at TIMESTAMPTZ NOT NULL default NOW(),

    FOREIGN KEY (source_room_id) REFERENCES room (id) ON DELETE CASCADE,
    PRIMARY KEY (id)
);

CREATE INDEX edition_source_room_id_idx ON edition (source_room_id);
