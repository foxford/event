CREATE TABLE IF NOT EXISTS room_ban (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    account_id account_id NOT NULL,
    room_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,

    PRIMARY KEY (id),
    UNIQUE (account_id, room_id),
    FOREIGN KEY (room_id) REFERENCES room(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS room_ban_idx ON room_ban USING btree (account_id, room_id);
